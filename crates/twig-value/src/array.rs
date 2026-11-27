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
//! Storage is an entry vector with tombstones plus, once the array grows past a few elements, the
//! hash of every entry and an open-addressing table of entry positions, so lookups, inserts and
//! removals are O(1), order is exact and a copy-on-write copy does not hash keys again.

use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::{Arc, LazyLock};

use crate::error::PhpError;
use crate::key::{ArrayKey, KeyRef};
use crate::string::PhpString;
use crate::value::Value;

/// `HT_MIN_SIZE`.
const HT_MIN_SIZE: u64 = 8;
/// Arrays up to this many entries are searched linearly; larger ones get a hash index.
const INDEX_THRESHOLD: usize = 8;
/// The spare entries a copy of a table gets (see `ArrayData::clone`).
const COPY_HEADROOM: usize = 4;

/// The hasher of array keys: one random seed per process, so that a key has the same hash in every array.
static KEY_HASHER: LazyLock<foldhash::fast::RandomState> = LazyLock::new(Default::default);

/// The hash of a normalised key in the index of an array.
fn key_hash(key: KeyRef<'_>) -> u32 {
    let mut hasher = KEY_HASHER.build_hasher();
    match key {
        KeyRef::Int(h) => hasher.write_u64(h as u64),
        KeyRef::Str(s) => hasher.write(s),
    }
    hasher.finish() as u32
}

/// Bytewise equality of two key strings. Variable names and other keys are short, and the comparisons of a linear
/// search mostly fail on the length or the first bytes: keys up to 16 bytes are compared with a few word loads instead
/// of a call of `memcmp`.
#[inline]
fn key_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    if n >= 8 {
        if n > 16 {
            return a == b;
        }
        let word =
            |s: &[u8], at: usize| u64::from_ne_bytes(s[at..at + 8].try_into().expect("8 bytes"));
        word(a, 0) == word(b, 0) && word(a, n - 8) == word(b, n - 8)
    } else if n >= 4 {
        let word =
            |s: &[u8], at: usize| u32::from_ne_bytes(s[at..at + 4].try_into().expect("4 bytes"));
        word(a, 0) == word(b, 0) && word(a, n - 4) == word(b, n - 4)
    } else if n > 0 {
        a[0] == b[0] && a[n / 2] == b[n / 2] && a[n - 1] == b[n - 1]
    } else {
        true
    }
}

/// Whether the stored key `stored` is `key` (see [`key_bytes_eq`]).
#[inline]
fn key_matches(stored: &ArrayKey, key: KeyRef<'_>) -> bool {
    match (stored, key) {
        (ArrayKey::Int(a), KeyRef::Int(b)) => *a == b,
        (ArrayKey::Str(a), KeyRef::Str(b)) => key_bytes_eq(a, b),
        _ => false,
    }
}

/// A free slot of an [`Index`].
const EMPTY_SLOT: u32 = u32::MAX;

/// The hash index of an array of more than [`INDEX_THRESHOLD`] entries: the hash of each entry (parallel to the
/// entries, stale for tombstones) and an open-addressing table (linear probing) of the positions of the live entries,
/// at most 5/8 full. Copying an index copies two vectors of integers.
struct Index {
    hashes: Vec<u32>,
    slots: Box<[u32]>,
}

impl Clone for Index {
    /// A copy with room for a few more hashes, like the entries of a copied table (see `ArrayData::clone`).
    fn clone(&self) -> Self {
        let mut hashes = Vec::with_capacity(self.hashes.len() + COPY_HEADROOM);
        hashes.extend_from_slice(&self.hashes);
        Index {
            hashes,
            slots: self.slots.clone(),
        }
    }
}

impl Index {
    /// An index of the live `entries`, whose hashes are `hashes`.
    fn build(entries: &[Option<(ArrayKey, Value)>], hashes: Vec<u32>, len: usize) -> Index {
        let mut index = Index {
            hashes,
            slots: vec![EMPTY_SLOT; (len * 2).next_power_of_two()].into_boxed_slice(),
        };
        for (pos, entry) in entries.iter().enumerate() {
            if entry.is_some() {
                index.insert_slot(pos);
            }
        }
        index
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// Whether `len` live entries fit at the maximum load.
    fn fits(&self, len: usize) -> bool {
        len * 8 <= self.slots.len() * 5
    }

    /// Records the entry at `pos` (its hash is already in `hashes`).
    fn insert_slot(&mut self, pos: usize) {
        let mask = self.mask();
        let mut i = self.hashes[pos] as usize & mask;
        while self.slots[i] != EMPTY_SLOT {
            i = (i + 1) & mask;
        }
        self.slots[i] = pos as u32;
    }

    /// The position of `key`, whose hash is `hash`, among `entries`.
    fn find(
        &self,
        entries: &[Option<(ArrayKey, Value)>],
        key: KeyRef<'_>,
        hash: u32,
    ) -> Option<usize> {
        let mask = self.mask();
        let mut i = hash as usize & mask;
        loop {
            let pos = self.slots[i];
            if pos == EMPTY_SLOT {
                return None;
            }
            let pos = pos as usize;
            if self.hashes[pos] == hash
                && matches!(&entries[pos], Some((k, _)) if key_matches(k, key))
            {
                return Some(pos);
            }
            i = (i + 1) & mask;
        }
    }

    /// Forgets the entry at `pos`: its slot is freed and the following slots of the probe run move back where their
    /// probe sequence allows (backward-shift deletion: no slot marks a deletion).
    fn remove_slot(&mut self, pos: usize) {
        let mask = self.mask();
        let mut i = self.hashes[pos] as usize & mask;
        while self.slots[i] != pos as u32 {
            i = (i + 1) & mask;
        }
        let mut j = i;
        loop {
            j = (j + 1) & mask;
            let moved = self.slots[j];
            if moved == EMPTY_SLOT {
                break;
            }
            let home = self.hashes[moved as usize] as usize & mask;
            // the entry at `j` stays when its home slot lies cyclically in (i, j]
            let stays = if i <= j {
                i < home && home <= j
            } else {
                i < home || home <= j
            };
            if !stays {
                self.slots[i] = moved;
                i = j;
            }
        }
        self.slots[i] = EMPTY_SLOT;
    }
}

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
            // No tombstones: the positions stay valid, so the index is copied as it is. A copy is mostly made to be
            // written (copy-on-write): room for a few more entries spares the first insert a reallocation.
            let mut entries = Vec::with_capacity(self.len + COPY_HEADROOM);
            entries.extend(self.entries.iter().cloned());
            return ArrayData {
                entries,
                index: self.index.clone(),
                len: self.len,
                next_free: self.next_free,
                layout: self.layout,
            };
        }
        let live = |pos: &usize| self.entries[*pos].is_some();
        let entries: Vec<Option<(ArrayKey, Value)>> = (0..self.entries.len())
            .filter(live)
            .map(|pos| self.entries[pos].clone())
            .collect();
        let hashes = self.index.as_ref().map(|index| {
            (0..self.entries.len())
                .filter(live)
                .map(|pos| index.hashes[pos])
                .collect()
        });
        let mut data = ArrayData {
            entries,
            index: None,
            len: self.len,
            next_free: self.next_free,
            layout: self.layout,
        };
        data.reindex(hashes);
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

    /// Builds the index of a table without tombstones that has more than [`INDEX_THRESHOLD`] entries (and drops it
    /// otherwise), from the hashes of the entries when they are known.
    fn reindex(&mut self, hashes: Option<Vec<u32>>) {
        if self.len <= INDEX_THRESHOLD {
            self.index = None;
            return;
        }
        let hashes = hashes.unwrap_or_else(|| {
            self.entries
                .iter()
                .map(|e| e.as_ref().map_or(0, |(k, _)| key_hash(k.as_key_ref())))
                .collect()
        });
        self.index = Some(Index::build(&self.entries, hashes, self.len));
    }

    /// Position of a key, without allocating.
    #[inline]
    fn find_ref(&self, key: KeyRef<'_>) -> Option<usize> {
        match &self.index {
            Some(index) => index.find(&self.entries, key, key_hash(key)),
            None => self
                .entries
                .iter()
                .position(|e| matches!(e, Some((k, _)) if key_matches(k, key))),
        }
    }

    #[inline]
    fn find(&self, key: &ArrayKey) -> Option<usize> {
        self.find_ref(key.as_key_ref())
    }

    #[inline]
    fn find_int(&self, h: i64) -> Option<usize> {
        self.find_ref(KeyRef::Int(h))
    }

    fn push_entry(&mut self, key: ArrayKey, value: Value, hash: Option<u32>) {
        let pos = self.entries.len();
        let hash = self
            .index
            .as_ref()
            .map(|_| hash.unwrap_or_else(|| key_hash(key.as_key_ref())));
        self.entries.push(Some((key, value)));
        self.len += 1;
        match (&mut self.index, hash) {
            (Some(index), Some(hash)) => {
                index.hashes.push(hash);
                if index.fits(self.len) {
                    index.insert_slot(pos);
                } else {
                    // grow: the positions stay, the slots are laid out again
                    let hashes = std::mem::take(&mut index.hashes);
                    self.index = Some(Index::build(&self.entries, hashes, self.len));
                }
            }
            _ => {
                if self.len > INDEX_THRESHOLD {
                    self.compact();
                }
            }
        }
    }

    /// Removes tombstones and rebuilds the index.
    fn compact(&mut self) {
        let mut hashes = self.index.take().map(|index| index.hashes);
        if self.entries.len() != self.len {
            if let Some(hashes) = &mut hashes {
                let mut pos = 0;
                hashes.retain(|_| {
                    pos += 1;
                    self.entries[pos - 1].is_some()
                });
            }
            self.entries.retain(Option::is_some);
        }
        self.reindex(hashes);
    }

    /// Insert of a new int key `h` (the caller checked that it does not exist): the layout part of
    /// `_zend_hash_index_add_or_update_i()` followed by the `nNextFreeElement` update.
    fn insert_new_int(&mut self, h: i64, value: Value, hash: Option<u32>) {
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
        self.push_entry(ArrayKey::Int(h), value, hash);
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

    fn insert_new_str(&mut self, key: PhpString, value: Value, hash: Option<u32>) {
        // zend_hash_real_init_mixed() / zend_hash_packed_to_hash()
        self.layout = Layout::Hash;
        self.push_entry(ArrayKey::Str(key), value, hash);
    }

    fn insert_new(&mut self, key: ArrayKey, value: Value) {
        self.insert_new_hashed(key, value, None);
    }

    /// Insert of a new key whose hash may be known.
    fn insert_new_hashed(&mut self, key: ArrayKey, value: Value, hash: Option<u32>) {
        match key {
            ArrayKey::Int(h) => self.insert_new_int(h, value, hash),
            ArrayKey::Str(s) => self.insert_new_str(s, value, hash),
        }
    }

    /// Position of a prepared key: the stored hash is the key's, and a string key sharing the prepared string is
    /// found without comparing bytes.
    #[inline]
    fn find_prepared(&self, key: &PreparedKey) -> Option<usize> {
        let matches = |entry: &Option<(ArrayKey, Value)>| prepared_matches(entry, key);
        match &self.index {
            Some(index) => {
                let mask = index.mask();
                let mut i = key.hash as usize & mask;
                loop {
                    let pos = index.slots[i];
                    if pos == EMPTY_SLOT {
                        return None;
                    }
                    let pos = pos as usize;
                    if index.hashes[pos] == key.hash && matches(&self.entries[pos]) {
                        return Some(pos);
                    }
                    i = (i + 1) & mask;
                }
            }
            None => self.entries.iter().position(matches),
        }
    }

    /// [`find_prepared`](Self::find_prepared) looking at position `hint` first; `hint` becomes the position found.
    #[inline]
    fn find_prepared_hinted(&self, key: &PreparedKey, hint: &mut u32) -> Option<usize> {
        let at = *hint as usize;
        if self
            .entries
            .get(at)
            .is_some_and(|entry| prepared_matches(entry, key))
        {
            return Some(at);
        }
        let pos = self.find_prepared(key)?;
        *hint = pos as u32;
        Some(pos)
    }

    /// [`insert_prepared`](Array::insert_prepared) on an unshared table, `hint` as in
    /// [`find_prepared_hinted`](Self::find_prepared_hinted).
    #[inline]
    fn insert_prepared_hinted(
        &mut self,
        key: &PreparedKey,
        value: Value,
        hint: &mut u32,
    ) -> Option<Value> {
        if let Some(pos) = self.find_prepared_hinted(key, hint) {
            let slot = self.entries[pos].as_mut().map(|(_, v)| v);
            return slot.map(|v| std::mem::replace(v, value));
        }
        *hint = self.entries.len() as u32;
        self.insert_new_hashed(key.key.clone(), value, Some(key.hash));
        None
    }

    fn remove_at(&mut self, pos: usize) -> Value {
        if let Some(index) = &mut self.index {
            index.remove_slot(pos);
        }
        let (key, value) = self.entries[pos]
            .take()
            .unwrap_or((ArrayKey::Int(0), Value::Null));
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
            if let Some(index) = &mut self.index {
                index.hashes.pop();
            }
        }
        if self.entries.len() > 2 * self.len + INDEX_THRESHOLD {
            self.compact();
        }
        value
    }
}

/// Whether the entry holds the prepared key: the same string (shared with the prepared key, or equal bytes) or int.
#[inline]
fn prepared_matches(entry: &Option<(ArrayKey, Value)>, key: &PreparedKey) -> bool {
    match (entry, &key.key) {
        (Some((ArrayKey::Str(a), _)), ArrayKey::Str(b)) => {
            PhpString::ptr_eq(a, b) || key_bytes_eq(a, b)
        }
        (Some((ArrayKey::Int(a), _)), ArrayKey::Int(b)) => a == b,
        _ => false,
    }
}

/// An array key normalised and hashed once, for repeated lookups in any array: the variable names of a compiled
/// template, for example ([`Array::get_prepared`], [`Array::insert_prepared`], [`Array::remove_prepared`]). The hash
/// is only valid in the process that computed it (arrays hash their keys with a seed chosen at startup).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedKey {
    key: ArrayKey,
    hash: u32,
}

impl PreparedKey {
    /// The prepared form of `key` (strings convert to keys with the symtable normalisation, like every
    /// [`ArrayKey`]).
    pub fn new(key: impl Into<ArrayKey>) -> PreparedKey {
        let key = key.into();
        let hash = key_hash(key.as_key_ref());
        PreparedKey { key, hash }
    }

    /// The key.
    #[inline]
    pub fn key(&self) -> &ArrayKey {
        &self.key
    }

    /// The key's hash (valid in this process only), to spread prepared keys over the slots of a cache of positions
    /// ([`Array::get_prepared_hinted`]).
    #[inline]
    pub fn hash(&self) -> u32 {
        self.hash
    }
}

/// Several writes to an array in a row ([`Array::edit`]): the array was separated when the edit started.
pub struct ArrayEdit<'a>(&'a mut ArrayData);

impl ArrayEdit<'_> {
    /// [`Array::get_prepared`].
    #[inline]
    pub fn get_prepared(&self, key: &PreparedKey) -> Option<&Value> {
        let pos = self.0.find_prepared(key)?;
        self.0.entries[pos].as_ref().map(|(_, v)| v)
    }

    /// [`Array::get_mut_prepared`].
    #[inline]
    pub fn get_mut_prepared(&mut self, key: &PreparedKey) -> Option<&mut Value> {
        let pos = self.0.find_prepared(key)?;
        self.0.entries[pos].as_mut().map(|(_, v)| v)
    }

    /// [`Array::insert_prepared`].
    #[inline]
    pub fn insert_prepared(&mut self, key: &PreparedKey, value: impl Into<Value>) -> Option<Value> {
        let value = value.into();
        if let Some(pos) = self.0.find_prepared(key) {
            let slot = self.0.entries[pos].as_mut().map(|(_, v)| v);
            return slot.map(|v| std::mem::replace(v, value));
        }
        self.0
            .insert_new_hashed(key.key.clone(), value, Some(key.hash));
        None
    }

    /// [`Array::remove_prepared`].
    #[inline]
    pub fn remove_prepared(&mut self, key: &PreparedKey) -> Option<Value> {
        let pos = self.0.find_prepared(key)?;
        Some(self.0.remove_at(pos))
    }

    /// [`Array::insert_prepared_hinted`].
    #[inline]
    pub fn insert_prepared_hinted(
        &mut self,
        key: &PreparedKey,
        value: impl Into<Value>,
        hint: &mut u32,
    ) -> Option<Value> {
        self.0.insert_prepared_hinted(key, value.into(), hint)
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
    #[inline]
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

    #[inline]
    fn data_mut(&mut self) -> &mut ArrayData {
        Arc::make_mut(&mut self.0)
    }

    /// Number of elements (`zend_hash_num_elements`, `count()`).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len
    }

    /// Whether the array is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.len == 0
    }

    /// Whether both arrays share the same table. PHP's `zend_is_identical()` and
    /// `zend_compare_symbol_tables()` short-circuit on this (engine-semantics.md §2.2).
    #[inline]
    pub fn ptr_eq(a: &Array, b: &Array) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The address of the shared table: an identity for recursion guards (`GC_PROTECT_RECURSION`
    /// on a `HashTable`), equal for arrays that are [`Array::ptr_eq`].
    pub(crate) fn table_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// Looks up a normalised key.
    #[inline]
    pub fn get(&self, key: &ArrayKey) -> Option<&Value> {
        self.get_ref(key.as_key_ref())
    }

    /// Looks up a borrowed, normalised key (no allocation).
    #[inline]
    pub fn get_ref(&self, key: KeyRef<'_>) -> Option<&Value> {
        let pos = self.0.find_ref(key)?;
        self.0.entries[pos].as_ref().map(|(_, v)| v)
    }

    /// Looks up an int key (`zend_hash_index_find`).
    #[inline]
    pub fn get_int(&self, h: i64) -> Option<&Value> {
        self.get_ref(KeyRef::Int(h))
    }

    /// Looks up a string key **without** numeric normalisation (`zend_hash_find`). Use
    /// [`Array::get_sym`] for PHP's `$a["..."]` semantics.
    #[inline]
    pub fn get_str(&self, key: &[u8]) -> Option<&Value> {
        self.get_ref(KeyRef::Str(key))
    }

    /// Looks up a string key with symtable normalisation (`zend_symtable_find`: `"1"` finds `1`).
    pub fn get_sym(&self, key: &[u8]) -> Option<&Value> {
        self.get_ref(KeyRef::sym(key))
    }

    /// Whether the key exists (`zend_hash_exists`; `null` values exist).
    #[inline]
    pub fn contains_key(&self, key: &ArrayKey) -> bool {
        self.0.find(key).is_some()
    }

    /// Whether the borrowed key exists.
    #[inline]
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
    #[inline]
    pub fn get_mut(&mut self, key: &ArrayKey) -> Option<&mut Value> {
        self.get_mut_ref(key.as_key_ref())
    }

    /// Mutable access to the element of a borrowed, normalised key (separates a shared array first, like
    /// [`Array::get_mut`]).
    #[inline]
    pub fn get_mut_ref(&mut self, key: KeyRef<'_>) -> Option<&mut Value> {
        let data = self.data_mut();
        let pos = data.find_ref(key)?;
        data.entries[pos].as_mut().map(|(_, v)| v)
    }

    /// Inserts or updates the element of a string key with symtable normalisation (`$a["name"] = $v`, like
    /// [`Array::insert`] with `ArrayKey::from_string(key.clone())`). The key is looked up borrowed: an existing key
    /// keeps its position and its string, and `key` is only cloned into the table when it is new. Returns the
    /// previous value.
    pub fn insert_sym(&mut self, key: &PhpString, value: impl Into<Value>) -> Option<Value> {
        let value = value.into();
        let key_ref = KeyRef::sym(key);
        let data = self.data_mut();
        if let Some(pos) = data.find_ref(key_ref) {
            let slot = data.entries[pos].as_mut().map(|(_, v)| v);
            return slot.map(|v| std::mem::replace(v, value));
        }
        let key = match key_ref {
            KeyRef::Int(h) => ArrayKey::Int(h),
            KeyRef::Str(_) => ArrayKey::Str(key.clone()),
        };
        data.insert_new(key, value);
        None
    }

    /// Looks up a prepared key ([`Array::get`] without normalising or hashing the key again).
    #[inline]
    pub fn get_prepared(&self, key: &PreparedKey) -> Option<&Value> {
        let pos = self.0.find_prepared(key)?;
        self.0.entries[pos].as_ref().map(|(_, v)| v)
    }

    /// [`Array::get_prepared`] looking at the position `hint` first, where the key was found before; `hint` becomes
    /// the position of the key. A frame that keeps the positions of its variables in its context finds them without
    /// searching (an array only grows at its end while nothing is removed). Any `u32` is a valid hint.
    #[inline]
    pub fn get_prepared_hinted(&self, key: &PreparedKey, hint: &mut u32) -> Option<&Value> {
        let pos = self.0.find_prepared_hinted(key, hint)?;
        self.0.entries[pos].as_ref().map(|(_, v)| v)
    }

    /// [`Array::insert_prepared`] with a position `hint` (see [`Array::get_prepared_hinted`]).
    #[inline]
    pub fn insert_prepared_hinted(
        &mut self,
        key: &PreparedKey,
        value: impl Into<Value>,
        hint: &mut u32,
    ) -> Option<Value> {
        self.data_mut()
            .insert_prepared_hinted(key, value.into(), hint)
    }

    /// Mutable access to the element of a prepared key (separates a shared array first, like [`Array::get_mut`]).
    #[inline]
    pub fn get_mut_prepared(&mut self, key: &PreparedKey) -> Option<&mut Value> {
        let data = self.data_mut();
        let pos = data.find_prepared(key)?;
        data.entries[pos].as_mut().map(|(_, v)| v)
    }

    /// Inserts or updates the element of a prepared key ([`Array::insert`]; an existing key keeps its position and
    /// its string, the prepared key is cloned into the table only when it is new). Returns the previous value.
    #[inline]
    pub fn insert_prepared(&mut self, key: &PreparedKey, value: impl Into<Value>) -> Option<Value> {
        let value = value.into();
        let data = self.data_mut();
        if let Some(pos) = data.find_prepared(key) {
            let slot = data.entries[pos].as_mut().map(|(_, v)| v);
            return slot.map(|v| std::mem::replace(v, value));
        }
        data.insert_new_hashed(key.key.clone(), value, Some(key.hash));
        None
    }

    /// Removes a prepared key ([`Array::remove`]: a shared array is separated even when the key is absent).
    #[inline]
    pub fn remove_prepared(&mut self, key: &PreparedKey) -> Option<Value> {
        let data = self.data_mut();
        let pos = data.find_prepared(key)?;
        Some(data.remove_at(pos))
    }

    /// Starts several writes in a row: a shared array is separated now, as the first write would separate it
    /// (`SEPARATE_ARRAY`), and the writes of the [`ArrayEdit`] go to the table without checking again whether it is
    /// shared.
    #[inline]
    pub fn edit(&mut self) -> ArrayEdit<'_> {
        ArrayEdit(self.data_mut())
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
        self.data_mut().insert_new_int(h, value.into(), None);
        Ok(h)
    }

    /// Removes a key (`unset($a[$k])`, `zend_hash_del`). Does not change [`Array::next_free`]. Like
    /// `ZEND_UNSET_DIM`, a shared array is separated even when the key is absent.
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Value> {
        self.remove_ref(key.as_key_ref())
    }

    /// Removes a borrowed, normalised key ([`Array::remove`] without building an [`ArrayKey`]; a shared array is
    /// separated even when the key is absent).
    pub fn remove_ref(&mut self, key: KeyRef<'_>) -> Option<Value> {
        let data = self.data_mut();
        let pos = data.find_ref(key)?;
        Some(data.remove_at(pos))
    }

    /// `nNextFreeElement`: the key the next [`Array::push`] would use, or `None` while no int key was
    /// ever inserted (`ZEND_LONG_MIN`; the next push uses `0`).
    #[inline]
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

    /// Random inserts, removals, appends and copies agree with a plain ordered list of entries, through the growth,
    /// compaction and shrinking of the index.
    #[test]
    fn random_operations_match_an_ordered_list() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let key = |n: u64| -> ArrayKey {
            if n.is_multiple_of(2) {
                ArrayKey::Int(n as i64 / 2 - 5)
            } else {
                ArrayKey::from(format!("k{}", n / 2))
            }
        };
        let mut arrays: Vec<(Array, Vec<(ArrayKey, Value)>)> = vec![(Array::new(), Vec::new())];
        for step in 0..20_000 {
            let which = next(arrays.len() as u64) as usize;
            let range = if step % 3000 < 1500 { 30 } else { 120 };
            let (array, model) = &mut arrays[which];
            match next(10) {
                0..=3 => {
                    let k = key(next(range));
                    let v = Value::Int(step);
                    if step % 2 == 0 {
                        array.insert(k.clone(), v.clone());
                    } else {
                        array.insert_prepared(&PreparedKey::new(k.clone()), v.clone());
                    }
                    match model.iter_mut().find(|(mk, _)| *mk == k) {
                        Some(entry) => entry.1 = v,
                        None => model.push((k, v)),
                    }
                }
                4..=6 => {
                    let k = key(next(range));
                    let removed = if step % 2 == 0 {
                        array.remove(&k)
                    } else {
                        array.remove_prepared(&PreparedKey::new(k.clone()))
                    };
                    let position = model.iter().position(|(mk, _)| *mk == k);
                    assert_eq!(removed, position.map(|p| model.remove(p).1));
                }
                7 => {
                    if let Ok(k) = array.push(Value::Int(-step)) {
                        model.push((ArrayKey::Int(k), Value::Int(-step)));
                    }
                }
                8 => {
                    if arrays.len() < 8 {
                        let copy = (arrays[which].0.clone(), arrays[which].1.clone());
                        arrays.push(copy);
                    }
                }
                _ => {
                    if arrays.len() > 1 {
                        arrays.swap_remove(which);
                    }
                }
            }
            let which = which.min(arrays.len() - 1);
            let (array, model) = &arrays[which];
            assert_eq!(array.len(), model.len());
            assert!(
                array
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .eq(model.iter().cloned())
            );
            for n in 0..range {
                let k = key(n);
                let expected = model.iter().find(|(mk, _)| *mk == k).map(|(_, v)| v);
                assert_eq!(array.get(&k), expected, "step {step}, key {k:?}");
                assert_eq!(array.get_prepared(&PreparedKey::new(k)), expected);
            }
        }
    }
}
