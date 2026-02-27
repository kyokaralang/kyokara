//! Built-in functions available to Kyokara programs.

use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;

use smallvec::SmallVec;

use crate::error::RuntimeError;
use crate::value::Value;

/// Stack-allocated argument vector for function calls.
pub type Args = SmallVec<[Value; 4]>;

/// Identifies an intrinsic function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicFn {
    // I/O
    Print,
    Println,
    IntToString,
    StringConcat,

    // List<T>
    ListNew,
    ListPush,
    ListLen,
    ListGet,
    ListHead,
    ListTail,
    ListIsEmpty,
    ListReverse,
    ListConcat,
    ListMap,
    ListFilter,
    ListFold,

    // Map<K,V>
    MapNew,
    MapInsert,
    MapGet,
    MapContains,
    MapRemove,
    MapLen,
    MapKeys,
    MapValues,
    MapIsEmpty,

    // String ops
    StringLen,
    StringContains,
    StringStartsWith,
    StringEndsWith,
    StringTrim,
    StringSplit,
    StringSubstring,
    StringToUpper,
    StringToLower,
    CharToString,

    // Int/Float math
    Abs,
    Min,
    Max,
    FloatAbs,
    FloatMin,
    FloatMax,
    IntToFloat,
    FloatToInt,
}

impl IntrinsicFn {
    /// Returns the capability required to call this intrinsic, if any.
    ///
    /// Only I/O intrinsics (print, println) require the "IO" capability.
    /// All other intrinsics are pure and require no capability.
    pub fn required_capability(self) -> Option<&'static str> {
        match self {
            IntrinsicFn::Print | IntrinsicFn::Println => Some("IO"),
            _ => None,
        }
    }

    /// Returns true if this intrinsic needs access to the interpreter
    /// (e.g. for calling closures or constructing Option values).
    pub fn needs_interpreter(self) -> bool {
        matches!(
            self,
            IntrinsicFn::ListGet
                | IntrinsicFn::ListHead
                | IntrinsicFn::ListMap
                | IntrinsicFn::ListFilter
                | IntrinsicFn::ListFold
                | IntrinsicFn::MapGet
        )
    }

    /// Execute the intrinsic with the given arguments.
    ///
    /// Complex intrinsics (where `needs_interpreter()` is true) are
    /// intercepted by the interpreter before reaching this method.
    pub fn call(self, args: Args) -> Result<Value, RuntimeError> {
        match self {
            // ── I/O ──────────────────────────────────────────────
            IntrinsicFn::Print => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "print expects a String argument".into(),
                    ));
                };
                print!("{s}");
                Ok(Value::Unit)
            }
            IntrinsicFn::Println => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "println expects a String argument".into(),
                    ));
                };
                println!("{s}");
                Ok(Value::Unit)
            }
            IntrinsicFn::IntToString => {
                let Value::Int(n) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "int_to_string expects an Int argument".into(),
                    ));
                };
                Ok(Value::String(n.to_string()))
            }
            IntrinsicFn::StringConcat => {
                let Value::String(a) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_concat expects String arguments".into(),
                    ));
                };
                let Value::String(b) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_concat expects String arguments".into(),
                    ));
                };
                Ok(Value::String(format!("{a}{b}")))
            }

            // ── List (simple) ────────────────────────────────────
            IntrinsicFn::ListNew => Ok(Value::List(Vec::new())),
            IntrinsicFn::ListPush => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "list_push: missing value argument".into(),
                ))?;
                let Value::List(mut xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "list_push: missing list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("list_push expects a List".into()));
                };
                xs.push(val);
                Ok(Value::List(xs))
            }
            IntrinsicFn::ListLen => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_len expects a List".into()));
                };
                Ok(Value::Int(xs.len() as i64))
            }
            IntrinsicFn::ListTail => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_tail expects a List".into()));
                };
                if xs.is_empty() {
                    Ok(Value::List(Vec::new()))
                } else {
                    Ok(Value::List(xs[1..].to_vec()))
                }
            }
            IntrinsicFn::ListIsEmpty => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_is_empty expects a List".into(),
                    ));
                };
                Ok(Value::Bool(xs.is_empty()))
            }
            IntrinsicFn::ListReverse => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_reverse expects a List".into(),
                    ));
                };
                let mut rev = xs.clone();
                rev.reverse();
                Ok(Value::List(rev))
            }
            IntrinsicFn::ListConcat => {
                let Value::List(a) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_concat expects List arguments".into(),
                    ));
                };
                let Value::List(b) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "list_concat expects List arguments".into(),
                    ));
                };
                let mut result = a.clone();
                result.extend(b.iter().cloned());
                Ok(Value::List(result))
            }

            // ── Map (simple) ─────────────────────────────────────
            IntrinsicFn::MapNew => Ok(Value::Map(Vec::new())),
            IntrinsicFn::MapInsert => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_insert expects a Map".into()));
                };
                let key = &args[1];
                let val = &args[2];
                let mut new_entries: Vec<(Value, Value)> =
                    entries.iter().filter(|(k, _)| k != key).cloned().collect();
                new_entries.push((key.clone(), val.clone()));
                Ok(Value::Map(new_entries))
            }
            IntrinsicFn::MapContains => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_contains expects a Map".into()));
                };
                let key = &args[1];
                Ok(Value::Bool(entries.iter().any(|(k, _)| k == key)))
            }
            IntrinsicFn::MapRemove => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_remove expects a Map".into()));
                };
                let key = &args[1];
                let new_entries: Vec<(Value, Value)> =
                    entries.iter().filter(|(k, _)| k != key).cloned().collect();
                Ok(Value::Map(new_entries))
            }
            IntrinsicFn::MapLen => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_len expects a Map".into()));
                };
                Ok(Value::Int(entries.len() as i64))
            }
            IntrinsicFn::MapKeys => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_keys expects a Map".into()));
                };
                Ok(Value::List(
                    entries.iter().map(|(k, _)| k.clone()).collect(),
                ))
            }
            IntrinsicFn::MapValues => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_values expects a Map".into()));
                };
                Ok(Value::List(
                    entries.iter().map(|(_, v)| v.clone()).collect(),
                ))
            }
            IntrinsicFn::MapIsEmpty => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_is_empty expects a Map".into()));
                };
                Ok(Value::Bool(entries.is_empty()))
            }

            // ── String ops ───────────────────────────────────────
            IntrinsicFn::StringLen => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_len expects a String".into(),
                    ));
                };
                Ok(Value::Int(s.chars().count() as i64))
            }
            IntrinsicFn::StringContains => {
                let Value::String(haystack) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_contains expects String arguments".into(),
                    ));
                };
                let Value::String(needle) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_contains expects String arguments".into(),
                    ));
                };
                Ok(Value::Bool(haystack.contains(needle.as_str())))
            }
            IntrinsicFn::StringStartsWith => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_starts_with expects String arguments".into(),
                    ));
                };
                let Value::String(prefix) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_starts_with expects String arguments".into(),
                    ));
                };
                Ok(Value::Bool(s.starts_with(prefix.as_str())))
            }
            IntrinsicFn::StringEndsWith => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_ends_with expects String arguments".into(),
                    ));
                };
                let Value::String(suffix) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_ends_with expects String arguments".into(),
                    ));
                };
                Ok(Value::Bool(s.ends_with(suffix.as_str())))
            }
            IntrinsicFn::StringTrim => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_trim expects a String".into(),
                    ));
                };
                Ok(Value::String(s.trim().to_string()))
            }
            IntrinsicFn::StringSplit => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_split expects String arguments".into(),
                    ));
                };
                let Value::String(delim) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_split expects String arguments".into(),
                    ));
                };
                let parts: Vec<Value> = s
                    .split(delim.as_str())
                    .map(|p| Value::String(p.to_string()))
                    .collect();
                Ok(Value::List(parts))
            }
            IntrinsicFn::StringSubstring => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_substring expects a String".into(),
                    ));
                };
                let Value::Int(start) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "string_substring expects Int indices".into(),
                    ));
                };
                let Value::Int(end) = &args[2] else {
                    return Err(RuntimeError::TypeError(
                        "string_substring expects Int indices".into(),
                    ));
                };
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let start = (*start).clamp(0, len) as usize;
                let end = (*end).clamp(0, len) as usize;
                let sub: String = if start <= end {
                    chars[start..end].iter().collect()
                } else {
                    String::new()
                };
                Ok(Value::String(sub))
            }
            IntrinsicFn::StringToUpper => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_to_upper expects a String".into(),
                    ));
                };
                Ok(Value::String(s.to_uppercase()))
            }
            IntrinsicFn::StringToLower => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_to_lower expects a String".into(),
                    ));
                };
                Ok(Value::String(s.to_lowercase()))
            }
            IntrinsicFn::CharToString => {
                let Value::Char(c) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "char_to_string expects a Char".into(),
                    ));
                };
                Ok(Value::String(c.to_string()))
            }

            // ── Int/Float math ───────────────────────────────────
            IntrinsicFn::Abs => {
                let Value::Int(n) = &args[0] else {
                    return Err(RuntimeError::TypeError("abs expects an Int".into()));
                };
                n.checked_abs()
                    .map(Value::Int)
                    .ok_or(RuntimeError::IntegerOverflow)
            }
            IntrinsicFn::Min => {
                let Value::Int(a) = &args[0] else {
                    return Err(RuntimeError::TypeError("min expects Int arguments".into()));
                };
                let Value::Int(b) = &args[1] else {
                    return Err(RuntimeError::TypeError("min expects Int arguments".into()));
                };
                Ok(Value::Int(*a.min(b)))
            }
            IntrinsicFn::Max => {
                let Value::Int(a) = &args[0] else {
                    return Err(RuntimeError::TypeError("max expects Int arguments".into()));
                };
                let Value::Int(b) = &args[1] else {
                    return Err(RuntimeError::TypeError("max expects Int arguments".into()));
                };
                Ok(Value::Int(*a.max(b)))
            }
            IntrinsicFn::FloatAbs => {
                let Value::Float(f) = &args[0] else {
                    return Err(RuntimeError::TypeError("float_abs expects a Float".into()));
                };
                Ok(Value::Float(f.abs()))
            }
            IntrinsicFn::FloatMin => {
                let Value::Float(a) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "float_min expects Float arguments".into(),
                    ));
                };
                let Value::Float(b) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "float_min expects Float arguments".into(),
                    ));
                };
                Ok(Value::Float(a.min(*b)))
            }
            IntrinsicFn::FloatMax => {
                let Value::Float(a) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "float_max expects Float arguments".into(),
                    ));
                };
                let Value::Float(b) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "float_max expects Float arguments".into(),
                    ));
                };
                Ok(Value::Float(a.max(*b)))
            }
            IntrinsicFn::IntToFloat => {
                let Value::Int(n) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "int_to_float expects an Int".into(),
                    ));
                };
                Ok(Value::Float(*n as f64))
            }
            IntrinsicFn::FloatToInt => {
                let Value::Float(f) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "float_to_int expects a Float".into(),
                    ));
                };
                Ok(Value::Int(*f as i64))
            }

            // ── Complex (intercepted by interpreter) ─────────────
            IntrinsicFn::ListGet
            | IntrinsicFn::ListHead
            | IntrinsicFn::ListMap
            | IntrinsicFn::ListFilter
            | IntrinsicFn::ListFold
            | IntrinsicFn::MapGet => Err(RuntimeError::TypeError(
                "complex intrinsic called without interpreter context".into(),
            )),
        }
    }
}

/// All intrinsic name-function pairs.
pub fn all_intrinsics(interner: &mut Interner) -> Vec<(Name, IntrinsicFn)> {
    vec![
        // I/O
        (Name::new(interner, "print"), IntrinsicFn::Print),
        (Name::new(interner, "println"), IntrinsicFn::Println),
        (
            Name::new(interner, "int_to_string"),
            IntrinsicFn::IntToString,
        ),
        (
            Name::new(interner, "string_concat"),
            IntrinsicFn::StringConcat,
        ),
        // List
        (Name::new(interner, "list_new"), IntrinsicFn::ListNew),
        (Name::new(interner, "list_push"), IntrinsicFn::ListPush),
        (Name::new(interner, "list_len"), IntrinsicFn::ListLen),
        (Name::new(interner, "list_get"), IntrinsicFn::ListGet),
        (Name::new(interner, "list_head"), IntrinsicFn::ListHead),
        (Name::new(interner, "list_tail"), IntrinsicFn::ListTail),
        (
            Name::new(interner, "list_is_empty"),
            IntrinsicFn::ListIsEmpty,
        ),
        (
            Name::new(interner, "list_reverse"),
            IntrinsicFn::ListReverse,
        ),
        (Name::new(interner, "list_concat"), IntrinsicFn::ListConcat),
        (Name::new(interner, "list_map"), IntrinsicFn::ListMap),
        (Name::new(interner, "list_filter"), IntrinsicFn::ListFilter),
        (Name::new(interner, "list_fold"), IntrinsicFn::ListFold),
        // Map
        (Name::new(interner, "map_new"), IntrinsicFn::MapNew),
        (Name::new(interner, "map_insert"), IntrinsicFn::MapInsert),
        (Name::new(interner, "map_get"), IntrinsicFn::MapGet),
        (
            Name::new(interner, "map_contains"),
            IntrinsicFn::MapContains,
        ),
        (Name::new(interner, "map_remove"), IntrinsicFn::MapRemove),
        (Name::new(interner, "map_len"), IntrinsicFn::MapLen),
        (Name::new(interner, "map_keys"), IntrinsicFn::MapKeys),
        (Name::new(interner, "map_values"), IntrinsicFn::MapValues),
        (Name::new(interner, "map_is_empty"), IntrinsicFn::MapIsEmpty),
        // String
        (Name::new(interner, "string_len"), IntrinsicFn::StringLen),
        (
            Name::new(interner, "string_contains"),
            IntrinsicFn::StringContains,
        ),
        (
            Name::new(interner, "string_starts_with"),
            IntrinsicFn::StringStartsWith,
        ),
        (
            Name::new(interner, "string_ends_with"),
            IntrinsicFn::StringEndsWith,
        ),
        (Name::new(interner, "string_trim"), IntrinsicFn::StringTrim),
        (
            Name::new(interner, "string_split"),
            IntrinsicFn::StringSplit,
        ),
        (
            Name::new(interner, "string_substring"),
            IntrinsicFn::StringSubstring,
        ),
        (
            Name::new(interner, "string_to_upper"),
            IntrinsicFn::StringToUpper,
        ),
        (
            Name::new(interner, "string_to_lower"),
            IntrinsicFn::StringToLower,
        ),
        (
            Name::new(interner, "char_to_string"),
            IntrinsicFn::CharToString,
        ),
        // Int/Float
        (Name::new(interner, "abs"), IntrinsicFn::Abs),
        (Name::new(interner, "min"), IntrinsicFn::Min),
        (Name::new(interner, "max"), IntrinsicFn::Max),
        (Name::new(interner, "float_abs"), IntrinsicFn::FloatAbs),
        (Name::new(interner, "float_min"), IntrinsicFn::FloatMin),
        (Name::new(interner, "float_max"), IntrinsicFn::FloatMax),
        (Name::new(interner, "int_to_float"), IntrinsicFn::IntToFloat),
        (Name::new(interner, "float_to_int"), IntrinsicFn::FloatToInt),
    ]
}
