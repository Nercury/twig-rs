//! PHP arrays (`HashTable`): insertion-ordered maps with int/string keys.
//!
//! reference/php/engine-semantics.md §8. Semantics reproduced from PHPSRC/Zend/zend_hash.c:
//!
//! * insertion order is iteration order; updating a key keeps its position; removing and
//!   re-inserting moves it to the end; integer keys are never sorted;
//! * `nNextFreeElement` ([`Array::next_free`]) drives `$a[] = v`: it starts "unset" (`ZEND_LONG_MIN`,
//!   first append uses `0`), every int insert of `h >= next` sets it to `h + 1` (negative keys count
//!   since PHP 8.3), removals never lower it, and an append at `PHP_INT_MAX` fails with
//!   `Cannot add element to the array as the next element is already occupied`;
//! * the packed/hash layout distinction is tracked because it is observable: inserting into a
//!   *packed* array at or beyond its used slots resets `nNextFreeElement` to `h + 1` even when that
//!   is lower than before (`$a = [0,1,2,3]; unset($a[3], $a[2]); $a[2] = 'y'; $a[] = 'z';` gives keys
//!   0..3, whereas the same steps on a hash array give 0, 1, 2, 4 — verified with PHP 8.5.13);
//! * copy-on-write: [`Array`] is an `Arc`; mutation separates a shared table first, with the
//!   `zend_array_dup()` rules (an emptied array becomes a fresh uninitialised table).
//!
//! Storage is an entry vector with tombstones plus a key index built once the array grows past a
//! few elements, so lookups, inserts and removals are O(1) and order is exact.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use crate::error::PhpError;
use crate::key::{ArrayKey, KeyRef, KeyView};
use crate::string::PhpString;
use crate::value::Value;

/// `HT_MIN_SIZE`.
const HT_MIN_SIZE: u64 = 8;
/// Arrays up to this many entries are searched linearly; larger ones get a hash index.
const INDEX_THRESHOLD: usize = 8;

type Index = HashMap<ArrayKey, usize, foldhash::fast::RandomState>;

/// Layout state that influences `nNextFreeElement` (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// `HASH_FLAG_UNINITIALIZED` with `nTableSize`.
    Uninit { table_size: u64 },
    /// `HASH_FLAG_PACKED`: slots `0..num_used` (`nNumUsed`), capacity `table_size` (`nTableSize`).
    Packed { num_used: u64, table_size: u64 },
    /// A real hash table.
    Hash,
}

struct ArrayData {
    entries: Vec<Option<(ArrayKey, Value)>>,
    index: Option<Index>,
    len: usize,
    next_free: i64,
    layout: Layout,
}

impl Clone for ArrayData {
    /// `zend_array_dup()` (PHPSRC/Zend/zend_hash.c): an empty source becomes an uninitialised table
    /// of `HT_MIN_SIZE` keeping `nNextFreeElement`; otherwise the layout is preserved.
    fn clone(&self) -> Self {
        if self.len == 0 {
            return ArrayData {
                entries: Vec::new(),
                index: None,
                len: 0,
                next_free: self.next_free,
                layout: Layout::Uninit {
                    table_size: HT_MIN_SIZE,
                },
            };
        }
        if self.entries.len() == self.len {
            // No tombstones: the positions stay valid, so the index is copied instead of rebuilt.
            return ArrayData {
                entries: self.entries.clone(),
                index: self.index.clone(),
                len: self.len,
                next_free: self.next_free,
                layout: self.layout,
            };
        }
        let entries: Vec<Option<(ArrayKey, Value)>> = self
            .entries
            .iter()
            .filter(|e| e.is_some())
            .cloned()
            .collect();
        let mut data = ArrayData {
            entries,
            index: None,
            len: self.len,
            next_free: self.next_free,
            layout: self.layout,
        };
        data.rebuild_index();
        data
    }
}

/// `zend_hash_check_size()`.
fn check_size(n: usize) -> u64 {
    let n = n as u64;
    if n <= HT_MIN_SIZE {
        HT_MIN_SIZE
    } else {
        n.next_power_of_two()
    }
}

impl ArrayData {
    fn new(table_size: u64) -> Self {
        ArrayData {
            entries: Vec::new(),
            index: None,
            len: 0,
            next_free: i64::MIN,
            layout: Layout::Uninit { table_size },
        }
    }

    fn rebuild_index(&mut self) {
        if self.len > INDEX_THRESHOLD {
            let mut index = Index::with_capacity_and_hasher(self.len, Default::default());
            for (pos, e) in self.entries.iter().enumerate() {
                if let Some((k, _)) = e {
                    index.insert(k.clone(), pos);
                }
            }
            self.index = Some(index);
        } else {
            self.index = None;
        }
    }

    /// Position of a key, without allocating (the index is queried through `Borrow<dyn KeyView>`).
    fn find_ref(&self, key: KeyRef<'_>) -> Option<usize> {
        match &self.index {
            Some(index) => index.get(&key as &dyn KeyView).copied(),
            None => self
                .entries
                .iter()
                .position(|e| matches!(e, Some((k, _)) if k.as_key_ref() == key)),
        }
    }

    fn find(&self, key: &ArrayKey) -> Option<usize> {
        self.find_ref(key.as_key_ref())
    }

    fn find_int(&self, h: i64) -> Option<usize> {
        self.find_ref(KeyRef::Int(h))
    }

    fn push_entry(&mut self, key: ArrayKey, value: Value) {
        let pos = self.entries.len();
        if let Some(index) = &mut self.index {
            index.insert(key.clone(), pos);
        }
        self.entries.push(Some((key, value)));
        self.len += 1;
        if self.index.is_none() && self.len > INDEX_THRESHOLD {
            self.compact();
        }
    }

    /// Removes tombstones and rebuilds the index.
    fn compact(&mut self) {
        if self.entries.len() != self.len {
            self.entries.retain(Option::is_some);
        }
        self.rebuild_index();
    }

    /// Insert of a new int key `h` (the caller checked that it does not exist): the layout part of
    /// `_zend_hash_index_add_or_update_i()` followed by the `nNextFreeElement` update.
    fn insert_new_int(&mut self, h: i64, value: Value) {
        let uh = h as u64;
        let packed = match self.layout {
            Layout::Uninit { table_size } => {
                if uh < table_size {
                    self.layout = Layout::Packed {
                        num_used: 0,
                        table_size,
                    };
                    true
                } else {
                    self.layout = Layout::Hash;
                    false
                }
            }
            Layout::Packed {
                num_used,
                table_size,
            } => {
                if uh < num_used {
                    // An UNDEF hole: "we have to keep the order" -> convert to hash.
                    self.layout = Layout::Hash;
                    false
                } else if uh < table_size {
                    true
                } else if (uh >> 1) < table_size && (table_size >> 1) < self.len as u64 {
                    self.layout = Layout::Packed {
                        num_used,
                        table_size: table_size * 2,
                    };
                    true
                } else {
                    self.layout = Layout::Hash;
                    false
                }
            }
            Layout::Hash => false,
        };
        self.push_entry(ArrayKey::Int(h), value);
        if packed {
            // add_to_packed: ht->nNextFreeElement = ht->nNumUsed = h + 1;
            if let Layout::Packed { table_size, .. } = self.layout {
                self.layout = Layout::Packed {
                    num_used: uh + 1,
                    table_size,
                };
            }
            self.next_free = h + 1;
        } else if h >= self.next_free {
            self.next_free = if h < i64::MAX { h + 1 } else { i64::MAX };
        }
    }

    fn insert_new_str(&mut self, key: PhpString, value: Value) {
        // zend_hash_real_init_mixed() / zend_hash_packed_to_hash()
        self.layout = Layout::Hash;
        self.push_entry(ArrayKey::Str(key), value);
    }

    fn insert_new(&mut self, key: ArrayKey, value: Value) {
        match key {
            ArrayKey::Int(h) => self.insert_new_int(h, value),
            ArrayKey::Str(s) => self.insert_new_str(s, value),
        }
    }

    fn remove_at(&mut self, pos: usize) -> Value {
        let (key, value) = self.entries[pos]
            .take()
            .unwrap_or((ArrayKey::Int(0), Value::Null));
        if let Some(index) = &mut self.index {
            index.remove(&key);
        }
        self.len -= 1;
        if let (
            Layout::Packed {
                num_used,
                table_size,
            },
            ArrayKey::Int(h),
        ) = (self.layout, &key)
            && (*h as u64).wrapping_add(1) == num_used
        {
            // _zend_hash_packed_del_val(): deleting the last used slot makes nNumUsed drop past the
            // trailing holes. Packed entries are in increasing key order, so the last live entry
            // holds the largest key.
            let new_used = self
                .entries
                .iter()
                .rev()
                .find_map(|e| {
                    e.as_ref()
                        .map(|(k, _)| k.as_int().map_or(0, |k| k as u64 + 1))
                })
                .unwrap_or(0);
            self.layout = Layout::Packed {
                num_used: new_used,
                table_size,
            };
        }
        // Drop trailing tombstones eagerly; compact when tombstones dominate.
        while matches!(self.entries.last(), Some(None)) {
            self.entries.pop();
        }
        if self.entries.len() > 2 * self.len + INDEX_THRESHOLD {
            self.compact();
        }
        value
    }
}

static EMPTY_ARRAY: LazyLock<Array> =
    LazyLock::new(|| Array(Arc::new(ArrayData::new(HT_MIN_SIZE))));

/// A PHP array (copy-on-write `HashTable`).
///
/// Cloning is O(1) and shares storage; the first mutation of a shared array copies it
/// (`SEPARATE_ARRAY`). `PartialEq` is PHP identity (`===`, see [`Array::identical`]).
#[derive(Clone)]
pub struct Array(Arc<ArrayData>);

impl Default for Array {
    fn default() -> Self {
        Array::new()
    }
}

impl Array {
    /// An empty array (`[]` / `array_init()`: uninitialised table of `HT_MIN_SIZE`). Empty arrays
    /// share one immutable table until written, like `zend_empty_array`.
    pub fn new() -> Array {
        EMPTY_ARRAY.clone()
    }

    /// An empty array sized for `n` elements (`array_init_size()` / `zend_new_array(n)`).
    pub fn with_capacity(n: usize) -> Array {
        Array(Arc::new(ArrayData::new(check_size(n))))
    }

    /// An empty array already initialised as a hash table (`zend_new_array()` +
    /// `zend_hash_real_init_mixed()`, what `ZEND_INIT_ARRAY` does for array literals with explicit
    /// keys).
    pub fn new_hash() -> Array {
        let mut data = ArrayData::new(HT_MIN_SIZE);
        data.layout = Layout::Hash;
        Array(Arc::new(data))
    }

    fn data_mut(&mut self) -> &mut ArrayData {
        Arc::make_mut(&mut self.0)
    }

    /// Number of elements (`zend_hash_num_elements`, `count()`).
    pub fn len(&self) -> usize {
        self.0.len
    }

    /// Whether the array is empty.
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }

    /// Whether both arrays share the same table. PHP's `zend_is_identical()` and
    /// `zend_compare_symbol_tables()` short-circuit on this (engine-semantics.md §2.2).
    pub fn ptr_eq(a: &Array, b: &Array) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The address of the shared table: an identity for recursion guards (`GC_PROTECT_RECURSION`
    /// on a `HashTable`), equal for arrays that are [`Array::ptr_eq`].
    pub(crate) fn table_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// Looks up a normalised key.
    pub fn get(&self, key: &ArrayKey) -> Option<&Value> {
        self.get_ref(key.as_key_ref())
    }

    /// Looks up a borrowed, normalised key (no allocation).
    pub fn get_ref(&self, key: KeyRef<'_>) -> Option<&Value> {
        let pos = self.0.find_ref(key)?;
        self.0.entries[pos].as_ref().map(|(_, v)| v)
    }

    /// Looks up an int key (`zend_hash_index_find`).
    pub fn get_int(&self, h: i64) -> Option<&Value> {
        self.get_ref(KeyRef::Int(h))
    }

    /// Looks up a string key **without** numeric normalisation (`zend_hash_find`). Use
    /// [`Array::get_sym`] for PHP's `$a["..."]` semantics.
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        self.get_ref(KeyRef::Str(key))
    }

    /// Looks up a string key with symtable normalisation (`zend_symtable_find`: `"1"` finds `1`).
    pub fn get_sym(&self, key: &[u8]) -> Option<&Value> {
        self.get_ref(KeyRef::sym(key))
    }

    /// Whether the key exists (`zend_hash_exists`; `null` values exist).
    pub fn contains_key(&self, key: &ArrayKey) -> bool {
        self.0.find(key).is_some()
    }

    /// Whether the borrowed key exists.
    pub fn contains_ref(&self, key: KeyRef<'_>) -> bool {
        self.0.find_ref(key).is_some()
    }

    /// The key and value at iteration position `n` (O(n) when the table has holes).
    pub fn get_index(&self, n: usize) -> Option<(&ArrayKey, &Value)> {
        if self.0.entries.len() == self.0.len {
            return self
                .0
                .entries
                .get(n)
                .and_then(|e| e.as_ref())
                .map(|(k, v)| (k, v));
        }
        self.iter().nth(n)
    }

    /// Mutable access to an element (separates a shared array first, like PHP's write fetch).
    pub fn get_mut(&mut self, key: &ArrayKey) -> Option<&mut Value> {
        let data = self.data_mut();
        let pos = data.find(key)?;
        data.entries[pos].as_mut().map(|(_, v)| v)
    }

    /// Inserts or updates (`zend_hash_update` / `zend_hash_index_update`, i.e. `$a[$k] = $v` and
    /// array-literal elements with a key). An existing key keeps its position. Returns the previous
    /// value.
    pub fn insert(&mut self, key: impl Into<ArrayKey>, value: impl Into<Value>) -> Option<Value> {
        let key = key.into();
        let value = value.into();
        let data = self.data_mut();
        if let Some(pos) = data.find(&key) {
            let slot = data.entries[pos].as_mut().map(|(_, v)| v);
            return slot.map(|v| std::mem::replace(v, value));
        }
        data.insert_new(key, value);
        None
    }

    /// Inserts only if the key is absent (`zend_hash_add` / `zend_hash_index_add`, used by the
    /// array union `+`). Returns whether the value was inserted.
    pub fn insert_if_absent(&mut self, key: impl Into<ArrayKey>, value: impl Into<Value>) -> bool {
        let key = key.into();
        if self.0.find(&key).is_some() {
            return false;
        }
        self.data_mut().insert_new(key, value.into());
        true
    }

    /// The element for `key`, inserting `null` first when absent (the write fetch `$a[$k][...] =`,
    /// `zend_hash_lookup` / `zend_hash_index_lookup`).
    pub fn get_or_insert_null(&mut self, key: impl Into<ArrayKey>) -> &mut Value {
        let key = key.into();
        let data = self.data_mut();
        let pos = match data.find(&key) {
            Some(pos) => pos,
            None => {
                data.insert_new(key.clone(), Value::Null);
                data.find(&key).unwrap_or(data.entries.len() - 1)
            }
        };
        match &mut data.entries[pos] {
            Some((_, v)) => v,
            None => unreachable!("index points at a live entry"),
        }
    }

    /// Appends with the next free integer key (`$a[] = $v`, `zend_hash_next_index_insert`). Returns
    /// the key used, or the PHP `Error` when the next key is already occupied (after a
    /// `PHP_INT_MAX` key).
    pub fn push(&mut self, value: impl Into<Value>) -> Result<i64, PhpError> {
        let next = self.0.next_free;
        let h = if next == i64::MIN { 0 } else { next };
        if self.0.find_int(h).is_some() {
            return Err(PhpError::error(
                "Cannot add element to the array as the next element is already occupied",
            ));
        }
        self.data_mut().insert_new_int(h, value.into());
        Ok(h)
    }

    /// Removes a key (`unset($a[$k])`, `zend_hash_del`). Does not change [`Array::next_free`]. Like
    /// `ZEND_UNSET_DIM`, a shared array is separated even when the key is absent.
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        let data = self.data_mut();
        let pos = data.find(key)?;
        Some(data.remove_at(pos))
    }

    /// `nNextFreeElement`: the key the next [`Array::push`] would use, or `None` while no int key was
    /// ever inserted (`ZEND_LONG_MIN`; the next push uses `0`).
    pub fn next_free(&self) -> Option<i64> {
        let n = self.0.next_free;
        if n == i64::MIN { None } else { Some(n) }
    }

    /// Sets `nNextFreeElement` directly (for builtins that recompute it, such as `array_pop()`,
    /// `array_shift()`, `array_splice()`). `None` means `ZEND_LONG_MIN`.
    pub fn set_next_free(&mut self, next: Option<i64>) {
        self.data_mut().next_free = next.unwrap_or(i64::MIN);
    }

    /// Port of `zend_array_is_list()` (`array_is_list()`): the keys are exactly `0, 1, 2, …` in
    /// iteration order (`[]` is a list).
    pub fn is_list(&self) -> bool {
        self.iter()
            .enumerate()
            .all(|(i, (k, _))| *k == ArrayKey::Int(i as i64))
    }

    /// The first element in iteration order.
    pub fn first(&self) -> Option<(&ArrayKey, &Value)> {
        self.iter().next()
    }

    /// The last element in iteration order.
    pub fn last(&self) -> Option<(&ArrayKey, &Value)> {
        self.iter().next_back()
    }

    /// Iterates over `(key, value)` pairs in order.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.0.entries.iter(),
            remaining: self.0.len,
        }
    }

    /// Iterates over the keys in order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &ArrayKey> + ExactSizeIterator {
        self.iter().map(|(k, _)| k)
    }

    /// Iterates over the values in order.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator {
        self.iter().map(|(_, v)| v)
    }

    /// Iterates over the values mutably, in order (separates a shared array first).
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.data_mut()
            .entries
            .iter_mut()
            .filter_map(|e| e.as_mut().map(|(_, v)| v))
    }

    /// Removes every element (keeps `nNextFreeElement`, like deleting each key).
    pub fn clear(&mut self) {
        let next = self.0.next_free;
        let mut data = ArrayData::new(HT_MIN_SIZE);
        data.next_free = next;
        data.layout = match self.0.layout {
            Layout::Packed { table_size, .. } => Layout::Packed {
                num_used: 0,
                table_size,
            },
            other => other,
        };
        self.0 = Arc::new(data);
    }

    /// Keeps only the elements for which `f` returns true (each removal behaves like `unset`).
    pub fn retain(&mut self, mut f: impl FnMut(&ArrayKey, &Value) -> bool) {
        let doomed: Vec<ArrayKey> = self
            .iter()
            .filter(|(k, v)| !f(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        for key in doomed {
            self.remove(&key);
        }
    }

    /// Port of `zend_is_identical()` for arrays: same table, or same size and, in order, equal keys
    /// and identical values (`zend_hash_compare(..., ordered = 1)`).
    pub fn identical(&self, other: &Array) -> bool {
        if Array::ptr_eq(self, other) {
            return true;
        }
        if self.len() != other.len() {
            return false;
        }
        self.iter()
            .zip(other.iter())
            .all(|((k1, v1), (k2, v2))| k1 == k2 && v1.identical(v2))
    }

    /// A list (`[v0, v1, …]`) from values, keys `0..n`.
    pub fn from_values(values: impl IntoIterator<Item = Value>) -> Array {
        let iter = values.into_iter();
        let mut a = Array::with_capacity(iter.size_hint().0);
        for v in iter {
            // Keys 0..n on a fresh array: the next key can never be occupied.
            let _ = a.push(v);
        }
        a
    }
}

impl PartialEq for Array {
    /// PHP identity (`===`).
    fn eq(&self, other: &Array) -> bool {
        self.identical(other)
    }
}

impl fmt::Debug for Array {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl FromIterator<Value> for Array {
    /// Builds a list (keys `0..n`).
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Array {
        Array::from_values(iter)
    }
}

impl<K: Into<ArrayKey>> FromIterator<(K, Value)> for Array {
    /// Builds an array with update semantics (later duplicates overwrite in place).
    fn from_iter<I: IntoIterator<Item = (K, Value)>>(iter: I) -> Array {
        let mut a = Array::new();
        for (k, v) in iter {
            a.insert(k, v);
        }
        a
    }
}

/// Iterator over `(key, value)` pairs of an [`Array`].
#[derive(Clone)]
pub struct Iter<'a> {
    inner: std::slice::Iter<'a, Option<(ArrayKey, Value)>>,
    remaining: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a ArrayKey, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        let (k, v) = self.inner.by_ref().flatten().next()?;
        self.remaining -= 1;
        Some((k, v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while let Some(e) = self.inner.next_back() {
            if let Some((k, v)) = e {
                self.remaining -= 1;
                return Some((k, v));
            }
        }
        None
    }
}

impl ExactSizeIterator for Iter<'_> {}

impl<'a> IntoIterator for &'a Array {
    type Item = (&'a ArrayKey, &'a Value);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// Owning iterator over `(key, value)` pairs (clones each element as it is reached when the table is shared).
pub struct IntoIter {
    inner: IntoIterInner,
}

enum IntoIterInner {
    /// The table was not shared: its entries are moved out.
    Owned(std::vec::IntoIter<Option<(ArrayKey, Value)>>),
    /// A shared (immutable) table, iterated by position.
    Shared { data: Arc<ArrayData>, pos: usize },
}

impl Iterator for IntoIter {
    type Item = (ArrayKey, Value);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            IntoIterInner::Owned(entries) => entries.by_ref().flatten().next(),
            IntoIterInner::Shared { data, pos } => {
                while let Some(entry) = data.entries.get(*pos) {
                    *pos += 1;
                    if let Some((k, v)) = entry {
                        return Some((k.clone(), v.clone()));
                    }
                }
                None
            }
        }
    }
}

impl IntoIterator for Array {
    type Item = (ArrayKey, Value);
    type IntoIter = IntoIter;

    fn into_iter(self) -> IntoIter {
        let inner = match Arc::try_unwrap(self.0) {
            Ok(data) => IntoIterInner::Owned(data.entries.into_iter()),
            Err(shared) => IntoIterInner::Shared {
                data: shared,
                pos: 0,
            },
        };
        IntoIter { inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(a: &Array) -> Vec<ArrayKey> {
        a.keys().cloned().collect()
    }

    fn ints(v: &[i64]) -> Vec<ArrayKey> {
        v.iter().map(|&i| ArrayKey::Int(i)).collect()
    }

    #[test]
    fn next_free_rules() {
        // [-5 => 'a', 'b'] -> -5, -4
        let mut a = Array::new();
        a.insert(-5, "a");
        assert_eq!(a.push("b").unwrap(), -4);
        // [-10 => 'a', -20 => 'b', 'c'] -> -9
        let mut a = Array::new();
        a.insert(-10, "a");
        a.insert(-20, "b");
        assert_eq!(a.push("c").unwrap(), -9);
        // [3 => 'a', 1 => 'b', 'c'] -> 4
        let mut a = Array::new();
        a.insert(3, "a");
        a.insert(1, "b");
        assert_eq!(a.push("c").unwrap(), 4);
        assert_eq!(keys(&a), ints(&[3, 1, 4]));
        // ['x' => 'a', 'b'] -> 0
        let mut a = Array::new();
        a.insert("x", "a");
        assert_eq!(a.push("b").unwrap(), 0);
        // [PHP_INT_MAX => 1] then append -> Error
        let mut a = Array::new();
        a.insert(i64::MAX, 1);
        assert_eq!(
            a.push(2).unwrap_err().message(),
            "Cannot add element to the array as the next element is already occupied"
        );
        // [PHP_INT_MIN => 'a', 'b'] -> PHP_INT_MIN + 1
        let mut a = Array::new();
        a.insert(i64::MIN, "a");
        assert_eq!(a.push("b").unwrap(), i64::MIN + 1);
        // unset does not reset: [1,2,3], unset(2), append -> 3
        let mut a: Array = [1, 2, 3].into_iter().map(Value::from).collect();
        a.remove(&ArrayKey::Int(2));
        assert_eq!(a.push(4).unwrap(), 3);
        // [5 => 1]; unset; copy; append -> 6
        let mut a = Array::new();
        a.insert(5, 1);
        a.remove(&ArrayKey::Int(5));
        let mut b = a.clone();
        assert_eq!(b.push(2).unwrap(), 6);
    }

    #[test]
    fn packed_quirks_match_php() {
        // Verified with PHP 8.5.13 (see tests/vectors/array_ops.json for the random sequences).
        let list = |n: i64| -> Array { (0..n).map(Value::from).collect() };
        let mut a = list(4);
        a.remove(&ArrayKey::Int(3));
        a.remove(&ArrayKey::Int(2));
        a.insert(2, "y");
        a.push("z").unwrap();
        assert_eq!(keys(&a), ints(&[0, 1, 2, 3]));

        let mut a = list(3);
        a.remove(&ArrayKey::Int(1));
        a.insert(1, "y");
        a.push("z").unwrap();
        assert_eq!(keys(&a), ints(&[0, 2, 1, 3]));

        let mut a = list(3);
        for k in 0..3 {
            a.remove(&ArrayKey::Int(k));
        }
        a.insert(0, "q");
        a.push("z").unwrap();
        assert_eq!(keys(&a), ints(&[0, 1]));

        let mut a = Array::new();
        a.insert("x", 1);
        for k in 0..4 {
            a.insert(k, k);
        }
        a.remove(&ArrayKey::Int(3));
        a.remove(&ArrayKey::Int(2));
        a.insert(2, "y");
        a.push("z").unwrap();
        assert_eq!(keys(&a)[1..], ints(&[0, 1, 2, 4])[..]);
    }

    #[test]
    fn order_and_cow() {
        let mut a = Array::new();
        a.insert(5, "x");
        a.insert(1, "y");
        a.push("z").unwrap();
        assert_eq!(keys(&a), ints(&[5, 1, 6]));
        let b = a.clone();
        assert!(Array::ptr_eq(&a, &b));
        a.insert(1, "Y");
        assert!(!Array::ptr_eq(&a, &b));
        assert_eq!(b.get_int(1), Some(&Value::from("y")));
        assert_eq!(a.get_int(1), Some(&Value::from("Y")));
        assert_eq!(keys(&a), ints(&[5, 1, 6]));
        a.remove(&ArrayKey::Int(5));
        a.insert(5, "again");
        assert_eq!(keys(&a), ints(&[1, 6, 5]));
        // literal duplicates: ['a'=>1,'b'=>2,'a'=>3]
        let a: Array = vec![
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
            ("a", Value::Int(3)),
        ]
        .into_iter()
        .collect();
        assert_eq!(keys(&a), vec![ArrayKey::from("a"), ArrayKey::from("b")]);
        assert_eq!(a.get_sym(b"a"), Some(&Value::Int(3)));
        assert_eq!(ArrayKey::from("10"), ArrayKey::Int(10));
    }

    #[test]
    fn large_arrays_and_lists() {
        let mut a = Array::new();
        for i in 0..1000 {
            a.insert(format!("k{i}"), i);
        }
        for i in (0..1000).step_by(2) {
            assert!(a.remove(&ArrayKey::from(format!("k{i}"))).is_some());
        }
        assert_eq!(a.len(), 500);
        assert_eq!(a.get(&ArrayKey::from("k1")), Some(&Value::Int(1)));
        assert_eq!(a.get(&ArrayKey::from("k2")), None);
        assert_eq!(
            a.first().map(|(k, _)| k.clone()),
            Some(ArrayKey::from("k1"))
        );
        assert_eq!(
            a.last().map(|(k, _)| k.clone()),
            Some(ArrayKey::from("k999"))
        );
        let l: Array = (0..20).map(Value::from).collect();
        assert!(l.is_list());
        assert!(Array::new().is_list());
        let mut m = l.clone();
        m.remove(&ArrayKey::Int(0));
        assert!(!m.is_list());
        assert_eq!(l.iter().len(), 20);
        let owned: Vec<(ArrayKey, Value)> = l.into_iter().collect();
        assert_eq!(owned.len(), 20);
    }

    #[test]
    fn identity() {
        let a: Array = vec![("a", Value::Int(1)), ("b", Value::Int(2))]
            .into_iter()
            .collect();
        let b: Array = vec![("b", Value::Int(2)), ("a", Value::Int(1))]
            .into_iter()
            .collect();
        assert!(!a.identical(&b));
        let c: Array = vec![("a", Value::Int(1)), ("b", Value::Int(2))]
            .into_iter()
            .collect();
        assert!(a.identical(&c));
        let n: Array = vec![Value::Float(f64::NAN)].into_iter().collect();
        assert!(n.identical(&n.clone()));
        let m: Array = vec![Value::Float(f64::NAN)].into_iter().collect();
        assert!(!n.identical(&m));
    }

    #[test]
    fn borrowed_lookups() {
        // Small (linear scan) and large (hash index) tables answer the same queries.
        for n in [4usize, 50] {
            let mut a = Array::new();
            for i in 0..n {
                a.insert(format!("k{i}"), i as i64);
                a.insert(i as i64 * 10, "int");
            }
            a.insert("7x", "str");
            assert_eq!(a.get_ref(KeyRef::Str(b"k3")), Some(&Value::Int(3)));
            assert_eq!(a.get_str(b"k3"), Some(&Value::Int(3)));
            assert_eq!(a.get_sym(b"30"), Some(&Value::from("int")));
            assert_eq!(
                a.get_str(b"30"),
                None,
                "no numeric normalisation in get_str"
            );
            assert_eq!(a.get_sym(b"7x"), Some(&Value::from("str")));
            assert!(a.contains_ref(KeyRef::Int(0)));
            assert!(!a.contains_ref(KeyRef::Str(b"k")));
            assert_eq!(KeyRef::sym(b"-5"), KeyRef::Int(-5));
            assert_eq!(KeyRef::sym(b"-0"), KeyRef::Str(b"-0"));
            assert_eq!(KeyRef::sym(b"x").to_owned_key(), ArrayKey::from("x"));
            assert_eq!(
                a.get_index(1).map(|(k, _)| k.clone()),
                Some(ArrayKey::Int(0))
            );
            a.remove(&ArrayKey::from("k0"));
            assert_eq!(
                a.get_index(0).map(|(k, _)| k.clone()),
                Some(ArrayKey::Int(0))
            );
            assert_eq!(a.get_index(a.len()), None);
        }
    }
}
