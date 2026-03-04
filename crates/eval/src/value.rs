//! Runtime value representation.

use std::collections::VecDeque;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::{ExprIdx, PatIdx};
use kyokara_hir_def::item_tree::{FnItemIdx, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::IntrinsicFn;

/// Map key — only types that are naturally hashable.
///
/// Mirrors Rust's own constraint: `HashMap<K, V>` requires `K: Hash + Eq`.
/// Floats, functions, lists, maps, ADTs, and records are not valid keys.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MapKey {
    Int(i64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
}

impl MapKey {
    /// Convert a `Value` to a `MapKey`, rejecting unhashable types.
    pub fn from_value(v: &Value) -> Result<Self, RuntimeError> {
        match v {
            Value::Int(n) => Ok(MapKey::Int(*n)),
            Value::String(s) => Ok(MapKey::String(s.clone())),
            Value::Char(c) => Ok(MapKey::Char(*c)),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Unit => Ok(MapKey::Unit),
            _ => Err(RuntimeError::TypeError(
                "unhashable type used as map key (only Int, String, Char, Bool, Unit are allowed)"
                    .into(),
            )),
        }
    }

    /// Convert back to a `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Int(n) => Value::Int(*n),
            MapKey::String(s) => Value::String(s.clone()),
            MapKey::Char(c) => Value::Char(*c),
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::Unit => Value::Unit,
        }
    }

    pub fn display(&self, _interner: &Interner) -> String {
        self.to_value().display(_interner)
    }
}

/// A runtime value.
///
/// Kept small (32 bytes) by boxing heap-heavy variants behind indirection.
/// This improves cache locality for the common Int/Float/Bool/Unit cases.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
    Adt {
        type_idx: TypeItemIdx,
        variant: usize,
        fields: Vec<Value>,
    },
    Record {
        fields: Vec<(Name, Value)>,
        /// Optional type index for named record types (used for method resolution).
        type_idx: Option<TypeItemIdx>,
    },
    List(Rc<Vec<Value>>),
    Deque(Rc<VecDeque<Value>>),
    Seq(Rc<SeqPlan>),
    Map(Rc<IndexMap<MapKey, Value>>),
    Set(Rc<IndexSet<MapKey>>),
    Fn(Box<FnValue>),
}

/// Function values — user-defined, lambdas, or intrinsics.
#[derive(Debug, Clone)]
pub enum FnValue {
    User(FnItemIdx),
    Lambda {
        params: Vec<PatIdx>,
        body_expr: ExprIdx,
        body: Rc<Body>,
        env: Env,
    },
    Intrinsic(IntrinsicFn),
    /// ADT constructor with fields (used when a constructor is passed as a value).
    Constructor {
        type_idx: TypeItemIdx,
        variant_idx: usize,
        arity: usize,
    },
}

/// Source generator for a lazy sequence.
#[derive(Debug, Clone)]
pub enum SeqSource {
    Range { start: i64, end: i64 },
    FromList(Rc<Vec<Value>>),
    FromDeque(Rc<VecDeque<Value>>),
    StringSplit { s: String, delim: String },
    StringLines { s: String },
    StringChars { s: String },
    MapKeys(Rc<IndexMap<MapKey, Value>>),
    MapValues(Rc<IndexMap<MapKey, Value>>),
    SetValues(Rc<IndexSet<MapKey>>),
}

/// Lazy, re-iterable sequence plan.
#[derive(Debug, Clone)]
pub enum SeqPlan {
    Source(SeqSource),
    Map {
        input: Rc<SeqPlan>,
        f: Value,
    },
    Filter {
        input: Rc<SeqPlan>,
        f: Value,
    },
    Scan {
        input: Rc<SeqPlan>,
        init: Value,
        f: Value,
    },
    Unfold {
        seed: Value,
        step: Value,
    },
    Enumerate {
        input: Rc<SeqPlan>,
    },
    Zip {
        left: Rc<SeqPlan>,
        right: Rc<SeqPlan>,
    },
    Chunks {
        input: Rc<SeqPlan>,
        n: i64,
    },
    Windows {
        input: Rc<SeqPlan>,
        n: i64,
    },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            (
                Value::Adt {
                    type_idx: t1,
                    variant: v1,
                    fields: f1,
                },
                Value::Adt {
                    type_idx: t2,
                    variant: v2,
                    fields: f2,
                },
            ) => t1 == t2 && v1 == v2 && f1 == f2,
            (Value::Record { fields: f1, .. }, Value::Record { fields: f2, .. }) => f1 == f2,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Deque(a), Value::Deque(b)) => a == b,
            // Sequences are lazy plans; do not force-evaluate for equality.
            (Value::Seq(_), Value::Seq(_)) => false,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            // Functions are never equal.
            (Value::Fn(_), Value::Fn(_)) => false,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Value {
    pub fn list(items: Vec<Value>) -> Self {
        Value::List(Rc::new(items))
    }

    pub fn seq_source(source: SeqSource) -> Self {
        Value::Seq(Rc::new(SeqPlan::Source(source)))
    }

    pub fn deque(items: VecDeque<Value>) -> Self {
        Value::Deque(Rc::new(items))
    }

    pub fn seq_plan(plan: SeqPlan) -> Self {
        Value::Seq(Rc::new(plan))
    }

    pub fn map(entries: IndexMap<MapKey, Value>) -> Self {
        Value::Map(Rc::new(entries))
    }

    pub fn set(entries: IndexSet<MapKey>) -> Self {
        Value::Set(Rc::new(entries))
    }

    pub fn display(&self, interner: &Interner) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Char(c) => c.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Unit => "()".to_string(),
            Value::Adt {
                variant, fields, ..
            } => {
                if fields.is_empty() {
                    format!("<variant {variant}>")
                } else {
                    let fs: Vec<String> = fields.iter().map(|f| f.display(interner)).collect();
                    format!("<variant {variant}>({})", fs.join(", "))
                }
            }
            Value::Record { fields, .. } => {
                let fs: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n.resolve(interner), v.display(interner)))
                    .collect();
                format!("{{ {} }}", fs.join(", "))
            }
            Value::List(items) => {
                let fs: Vec<String> = items.iter().map(|v| v.display(interner)).collect();
                format!("[{}]", fs.join(", "))
            }
            Value::Deque(items) => {
                let fs: Vec<String> = items.iter().map(|v| v.display(interner)).collect();
                format!("Deque([{}])", fs.join(", "))
            }
            Value::Seq(_) => "<seq>".to_string(),
            Value::Map(entries) => {
                let fs: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.display(interner), v.display(interner)))
                    .collect();
                format!("{{{}}}", fs.join(", "))
            }
            Value::Set(entries) => {
                let fs: Vec<String> = entries.iter().map(|k| k.display(interner)).collect();
                format!("#{{{}}}", fs.join(", "))
            }
            Value::Fn(_) => "<function>".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_clone_shares_storage_for_cow() {
        let original = Value::list(vec![Value::Int(1), Value::Int(2)]);
        let cloned = original.clone();
        let (Value::List(a), Value::List(b)) = (&original, &cloned) else {
            panic!("expected list values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "list clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn map_clone_shares_storage_for_cow() {
        let mut m = IndexMap::new();
        m.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(m);
        let cloned = original.clone();
        let (Value::Map(a), Value::Map(b)) = (&original, &cloned) else {
            panic!("expected map values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "map clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn deque_clone_shares_storage_for_cow() {
        let mut q = VecDeque::new();
        q.push_back(Value::Int(1));
        q.push_back(Value::Int(2));
        let original = Value::deque(q);
        let cloned = original.clone();
        let (Value::Deque(a), Value::Deque(b)) = (&original, &cloned) else {
            panic!("expected deque values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "deque clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn set_clone_shares_storage_for_cow() {
        let mut s = IndexSet::new();
        s.insert(MapKey::Int(1));
        let original = Value::set(s);
        let cloned = original.clone();
        let (Value::Set(a), Value::Set(b)) = (&original, &cloned) else {
            panic!("expected set values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "set clone should share storage before mutation in COW model"
        );
    }
}
