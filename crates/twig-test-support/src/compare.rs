//! Structural comparison of typed trees, with object identities matched up to renaming.
//!
//! Two trees produced by different PHP processes (or by this crate's Rust encoder) describe the
//! same values when they agree node by node, where object ids only have to correspond one to one.
//! An object is encoded in full where the encoder first met it and as a `ref` elsewhere; where that
//! is depends on which fields the encoder writes (the supplement's `storage` field meets objects
//! before the context does), so the comparison is placement independent: `object` and `ref` nodes
//! both resolve to the object's definition, ids are paired, and each pair of definitions is
//! compared once.

use std::collections::{HashMap, HashSet};

use crate::json::ErrorInfo;
use crate::typed::{Item, ObjectNode, TypedNode};

/// What a comparison looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompareOptions {
    /// Compare the extra object fields of `fixture_data.gen.php` (`methods`, `parents`,
    /// `interfaces_raw`, `props_hidden`, `flags`, `iterator_class`, `storage`, `inner`, `accepts`). When false they are
    /// ignored, which compares a rich tree with a corpus tree.
    pub extras: bool,
    /// Ignore the `datetime` and `timestamp` of `DateTimeInterface` nodes (fixture DATA such as
    /// `new \DateTimeImmutable('now')` differs between oracle runs).
    pub ignore_volatile_dates: bool,
    /// Classes whose `interfaces_raw` are compared as sets (known order differences).
    pub unordered_interfaces_for: &'static [&'static str],
}

/// Compares two trees; `Err` describes the first difference with its path.
pub fn compare_trees(
    expected: &TypedNode,
    actual: &TypedNode,
    options: CompareOptions,
) -> Result<(), String> {
    Comparer::new(options).compare(expected, actual)
}

/// A comparison session: id pairings and definitions persist across calls, so a sequence of
/// trees (for example probe results that refer to objects of a context tree) can be compared
/// consistently.
pub struct Comparer {
    options: CompareOptions,
    forward: HashMap<i64, i64>,
    backward: HashMap<i64, i64>,
    defs_expected: HashMap<i64, TypedNode>,
    defs_actual: HashMap<i64, TypedNode>,
    compared: HashSet<(i64, i64)>,
}

fn collect_defs(n: &TypedNode, defs: &mut HashMap<i64, TypedNode>) {
    n.walk(&mut |node| {
        let id = match node {
            TypedNode::Object(o) => o.id,
            TypedNode::Enum(e) => e.id,
            TypedNode::Closure { id, .. } => *id,
            _ => return,
        };
        defs.entry(id).or_insert_with(|| node.clone());
    });
}

/// The id and class of an object-like node (`object`, `enum`, `closure`, `ref`).
fn identity(n: &TypedNode) -> Option<(i64, &str)> {
    match n {
        TypedNode::Object(o) => Some((o.id, o.class.as_str())),
        TypedNode::Enum(e) => Some((e.id, e.class.as_str())),
        TypedNode::Closure { id, class } | TypedNode::Ref { id, class } => {
            Some((*id, class.as_str()))
        }
        _ => None,
    }
}

impl Comparer {
    /// A new session.
    pub fn new(options: CompareOptions) -> Comparer {
        Comparer {
            options,
            forward: HashMap::new(),
            backward: HashMap::new(),
            defs_expected: HashMap::new(),
            defs_actual: HashMap::new(),
            compared: HashSet::new(),
        }
    }

    /// Pairs two ids up front (an object both sides already know).
    pub fn pair(&mut self, expected: i64, actual: i64) -> Result<(), String> {
        self.pair_ids(expected, actual, "$")
    }

    /// Records the object definitions of trees that are not compared themselves (for example the
    /// context tree that later probe results refer to).
    pub fn learn(&mut self, expected: &TypedNode, actual: &TypedNode) {
        collect_defs(expected, &mut self.defs_expected);
        collect_defs(actual, &mut self.defs_actual);
    }

    /// Compares two trees within this session.
    pub fn compare(&mut self, expected: &TypedNode, actual: &TypedNode) -> Result<(), String> {
        self.learn(expected, actual);
        self.node(expected, actual, &mut String::from("$"))
    }

    fn pair_ids(&mut self, e: i64, a: i64, path: &str) -> Result<(), String> {
        match (self.forward.get(&e), self.backward.get(&a)) {
            (None, None) => {
                self.forward.insert(e, a);
                self.backward.insert(a, e);
                Ok(())
            }
            (Some(&x), Some(&y)) if x == a && y == e => Ok(()),
            _ => Err(format!("{path}: object #{e} does not correspond to #{a}")),
        }
    }

    fn node(&mut self, e: &TypedNode, a: &TypedNode, path: &mut String) -> Result<(), String> {
        if let (Some((ei, ec)), Some((ai, ac))) = (identity(e), identity(a)) {
            if ec != ac {
                return Err(format!("{path}: expected class {ec:?}, got {ac:?}"));
            }
            self.pair_ids(ei, ai, path)?;
            if !self.compared.insert((ei, ai)) {
                return Ok(());
            }
            let de = match e {
                TypedNode::Ref { .. } => self.defs_expected.get(&ei).cloned(),
                other => Some(other.clone()),
            };
            let da = match a {
                TypedNode::Ref { .. } => self.defs_actual.get(&ai).cloned(),
                other => Some(other.clone()),
            };
            return match (de, da) {
                (Some(de), Some(da)) => self.definition(&de, &da, path),
                (None, None) => Ok(()),
                _ => Err(format!(
                    "{path}: object #{ei}/#{ai} is defined on one side only"
                )),
            };
        }
        let mismatch = |what: &str| -> Result<(), String> {
            Err(format!(
                "{path}: {what}: expected {}, got {}",
                short(e),
                short(a)
            ))
        };
        match (e, a) {
            (TypedNode::Null, TypedNode::Null)
            | (TypedNode::Truncated, TypedNode::Truncated)
            | (TypedNode::Unknown, TypedNode::Unknown) => Ok(()),
            (TypedNode::Bool(x), TypedNode::Bool(y)) if x == y => Ok(()),
            (TypedNode::Int(x), TypedNode::Int(y)) if x == y => Ok(()),
            (TypedNode::Float(x), TypedNode::Float(y))
                if x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()) =>
            {
                Ok(())
            }
            (TypedNode::String(x), TypedNode::String(y)) if x == y => Ok(()),
            (TypedNode::Array(x), TypedNode::Array(y)) => self.items(x, y, path),
            (
                TypedNode::Resource { resource_type: t1 },
                TypedNode::Resource { resource_type: t2 },
            ) if t1 == t2 => Ok(()),
            _ => mismatch("value"),
        }
    }

    /// Compares two definitions of paired objects.
    fn definition(
        &mut self,
        e: &TypedNode,
        a: &TypedNode,
        path: &mut String,
    ) -> Result<(), String> {
        match (e, a) {
            (TypedNode::Object(x), TypedNode::Object(y)) => self.object(x, y, path),
            (TypedNode::Enum(x), TypedNode::Enum(y)) => {
                if x.case != y.case {
                    return Err(format!("{path}: expected case {}, got {}", x.case, y.case));
                }
                match (&x.value, &y.value) {
                    (None, None) => Ok(()),
                    (Some(p), Some(q)) => self.node(p, q, path),
                    _ => Err(format!("{path}: enum value present on one side only")),
                }
            }
            (TypedNode::Closure { .. }, TypedNode::Closure { .. }) => Ok(()),
            _ => Err(format!("{path}: expected {}, got {}", short(e), short(a))),
        }
    }

    fn items(&mut self, e: &[Item], a: &[Item], path: &mut String) -> Result<(), String> {
        if e.len() != a.len() {
            return Err(format!(
                "{path}: expected {} entries, got {}",
                e.len(),
                a.len()
            ));
        }
        for (x, y) in e.iter().zip(a) {
            if x.key != y.key {
                return Err(format!("{path}: expected key {}, got key {}", x.key, y.key));
            }
            let len = path.len();
            path.push_str(&format!("[{}]", x.key));
            self.node(&x.value, &y.value, path)?;
            path.truncate(len);
        }
        Ok(())
    }

    fn opt_node(
        &mut self,
        field: &str,
        e: &Option<TypedNode>,
        a: &Option<TypedNode>,
        path: &mut String,
    ) -> Result<(), String> {
        match (e, a) {
            (None, None) => Ok(()),
            (Some(x), Some(y)) => {
                let len = path.len();
                path.push('.');
                path.push_str(field);
                self.node(x, y, path)?;
                path.truncate(len);
                Ok(())
            }
            _ => Err(format!("{path}: field {field} present on one side only")),
        }
    }

    fn opt_items(
        &mut self,
        field: &str,
        e: &Option<Vec<Item>>,
        a: &Option<Vec<Item>>,
        path: &mut String,
    ) -> Result<(), String> {
        match (e, a) {
            (None, None) => Ok(()),
            (Some(x), Some(y)) => {
                let len = path.len();
                path.push('.');
                path.push_str(field);
                self.items(x, y, path)?;
                path.truncate(len);
                Ok(())
            }
            _ => Err(format!("{path}: field {field} present on one side only")),
        }
    }

    fn object(&mut self, e: &ObjectNode, a: &ObjectNode, path: &mut String) -> Result<(), String> {
        let p = path.clone();
        let eq = |field: &str, same: bool| -> Result<(), String> {
            if same {
                Ok(())
            } else {
                Err(format!("{p}: field {field} differs"))
            }
        };
        self.opt_node("content", &e.content, &a.content, path)?;
        eq("charset", e.charset == a.charset)?;
        let dates_volatile = self.options.ignore_volatile_dates;
        eq("datetime", dates_volatile || e.datetime == a.datetime)?;
        eq("timezone", e.timezone == a.timezone)?;
        eq("timestamp", dates_volatile || e.timestamp == a.timestamp)?;
        eq("name", e.name == a.name)?;
        eq("spec", e.spec == a.spec)?;
        eq("template", e.template == a.template)?;
        self.opt_node("entries", &e.entries, &a.entries, path)?;
        eq("xml", e.xml == a.xml)?;
        eq("interfaces", e.interfaces == a.interfaces)?;
        self.opt_node("toString", &e.to_string, &a.to_string, path)?;
        match (&e.to_string_error, &a.to_string_error) {
            (None, None) => {}
            (Some(x), Some(y)) => eq("toString_error", same_error(x, y))?,
            _ => eq("toString_error", false)?,
        }
        self.opt_items("props", &e.props, &a.props, path)?;
        if self.options.extras {
            eq("methods", e.methods == a.methods)?;
            eq("parents", e.parents == a.parents)?;
            let unordered = self
                .options
                .unordered_interfaces_for
                .contains(&e.display_class());
            let sorted = |v: &Option<Vec<String>>| {
                v.clone().map(|mut v| {
                    v.sort();
                    v
                })
            };
            eq(
                "interfaces_raw",
                if unordered {
                    sorted(&e.interfaces_raw) == sorted(&a.interfaces_raw)
                } else {
                    e.interfaces_raw == a.interfaces_raw
                },
            )?;
            self.opt_items("props_hidden", &e.props_hidden, &a.props_hidden, path)?;
            eq("flags", e.flags == a.flags)?;
            eq("iterator_class", e.iterator_class == a.iterator_class)?;
            match (&e.storage, &a.storage) {
                (None, None) => {}
                (Some(x), Some(y)) if x.len() == y.len() => {
                    for ((o1, i1), (o2, i2)) in x.iter().zip(y) {
                        self.node(o1, o2, path)?;
                        self.node(i1, i2, path)?;
                    }
                }
                _ => eq("storage", false)?,
            }
            self.opt_node("inner", &e.inner, &a.inner, path)?;
            eq("accepts", e.accepts == a.accepts)?;
        }
        Ok(())
    }
}

fn same_error(x: &ErrorInfo, y: &ErrorInfo) -> bool {
    x.class == y.class && x.message_bytes() == y.message_bytes()
}

fn short(n: &TypedNode) -> String {
    match n {
        TypedNode::Bool(b) => format!("bool({b})"),
        TypedNode::Int(i) => format!("int({i})"),
        TypedNode::Float(d) => format!("float({d:?})"),
        TypedNode::String(s) => format!("string({:?})", s.to_string_lossy()),
        TypedNode::Object(o) => format!("object({})", o.display_class()),
        TypedNode::Ref { class, id } => format!("ref({class}#{id})"),
        other => other.tag().to_string(),
    }
}
