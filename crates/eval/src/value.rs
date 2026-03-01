//! Runtime value representation.

use std::rc::Rc;

use indexmap::IndexMap;
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
    List(Vec<Value>),
    Map(Box<IndexMap<MapKey, Value>>),
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
            (Value::Map(a), Value::Map(b)) => a == b,
            // Functions are never equal.
            (Value::Fn(_), Value::Fn(_)) => false,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Value {
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
            Value::Map(entries) => {
                let fs: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.display(interner), v.display(interner)))
                    .collect();
                format!("{{{}}}", fs.join(", "))
            }
            Value::Fn(_) => "<function>".to_string(),
        }
    }
}
