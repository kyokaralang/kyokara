//! Built-in functions available to Kyokara programs.

use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;
use smallvec::SmallVec;

use crate::error::RuntimeError;
use crate::value::{MapKey, Value};

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
    ListRange,
    ListEnumerate,
    ListZip,
    ListChunks,
    ListWindows,

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
    // Set<T>
    SetNew,
    SetInsert,
    SetContains,
    SetRemove,
    SetLen,
    SetIsEmpty,
    SetValues,
    // Result<T, E>
    ResultUnwrapOr,
    ResultMapOr,

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
    Gcd,
    Lcm,
    FloatAbs,
    FloatMin,
    FloatMax,
    IntToFloat,
    FloatToInt,

    // Parsing
    ParseInt,
    ParseFloat,

    // String decomposition
    StringLines,
    StringChars,

    // File I/O
    ReadFile,
    ReadLine,
    ReadStdin,

    // Sorting
    ListSort,
    ListSortBy,
    ListBinarySearch,
}

impl IntrinsicFn {
    /// Returns the capability required to call this intrinsic, if any.
    ///
    /// Only I/O intrinsics (print, println) require the "io" capability.
    /// All other intrinsics are pure and require no capability.
    pub fn required_capability(self) -> Option<&'static str> {
        match self {
            IntrinsicFn::Print
            | IntrinsicFn::Println
            | IntrinsicFn::ReadLine
            | IntrinsicFn::ReadStdin => Some("io"),
            IntrinsicFn::ReadFile => Some("fs"),
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
                | IntrinsicFn::ListEnumerate
                | IntrinsicFn::ListZip
                | IntrinsicFn::MapGet
                | IntrinsicFn::ListSortBy
                | IntrinsicFn::ResultUnwrapOr
                | IntrinsicFn::ResultMapOr
                | IntrinsicFn::ParseInt
                | IntrinsicFn::ParseFloat
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
            IntrinsicFn::ListNew => Ok(Value::list(Vec::new())),
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
                Rc::make_mut(&mut xs).push(val);
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
                    Ok(Value::list(Vec::new()))
                } else {
                    Ok(Value::list(xs[1..].to_vec()))
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
                let mut rev = xs.as_ref().clone();
                rev.reverse();
                Ok(Value::list(rev))
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
                let mut result = a.as_ref().clone();
                result.extend(b.iter().cloned());
                Ok(Value::list(result))
            }
            IntrinsicFn::ListRange => {
                let Value::Int(start) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_range expects Int arguments".into(),
                    ));
                };
                let Value::Int(end) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "list_range expects Int arguments".into(),
                    ));
                };
                if start >= end {
                    return Ok(Value::list(Vec::new()));
                }
                let values: Vec<Value> = (*start..*end).map(Value::Int).collect();
                Ok(Value::list(values))
            }
            IntrinsicFn::ListChunks => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_chunks expects a List".into()));
                };
                let Value::Int(n) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "list_chunks expects an Int chunk size".into(),
                    ));
                };
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "list_chunks: chunk size must be > 0".into(),
                    ));
                }
                let chunk_size = *n as usize;
                let mut chunks = Vec::new();
                let mut i = 0usize;
                while i < xs.len() {
                    let end = usize::min(i + chunk_size, xs.len());
                    chunks.push(Value::list(xs[i..end].to_vec()));
                    i += chunk_size;
                }
                Ok(Value::list(chunks))
            }
            IntrinsicFn::ListWindows => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_windows expects a List".into(),
                    ));
                };
                let Value::Int(n) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "list_windows expects an Int window size".into(),
                    ));
                };
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "list_windows: window size must be > 0".into(),
                    ));
                }
                let window_size = *n as usize;
                if window_size > xs.len() {
                    return Ok(Value::list(Vec::new()));
                }
                let mut windows = Vec::with_capacity(xs.len() - window_size + 1);
                for i in 0..=(xs.len() - window_size) {
                    windows.push(Value::list(xs[i..(i + window_size)].to_vec()));
                }
                Ok(Value::list(windows))
            }

            // ── Map (simple) ─────────────────────────────────────
            IntrinsicFn::MapNew => Ok(Value::map(IndexMap::new())),
            IntrinsicFn::MapInsert => {
                let mut args = args;
                let value = args.pop().ok_or(RuntimeError::TypeError(
                    "map_insert: missing value argument".into(),
                ))?;
                let key_value = args.pop().ok_or(RuntimeError::TypeError(
                    "map_insert: missing key argument".into(),
                ))?;
                let Value::Map(mut entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "map_insert: missing map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("map_insert expects a Map".into()));
                };
                let key = MapKey::from_value(&key_value)?;
                if entries.get(&key) == Some(&value) {
                    return Ok(Value::Map(entries));
                }
                Rc::make_mut(&mut entries).insert(key, value);
                Ok(Value::Map(entries))
            }
            IntrinsicFn::MapContains => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_contains expects a Map".into()));
                };
                let key = MapKey::from_value(&args[1])?;
                Ok(Value::Bool(entries.contains_key(&key)))
            }
            IntrinsicFn::MapRemove => {
                let mut args = args;
                let key_value = args.pop().ok_or(RuntimeError::TypeError(
                    "map_remove: missing key argument".into(),
                ))?;
                let Value::Map(mut entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "map_remove: missing map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("map_remove expects a Map".into()));
                };
                let key = MapKey::from_value(&key_value)?;
                if !entries.contains_key(&key) {
                    return Ok(Value::Map(entries));
                }
                Rc::make_mut(&mut entries).shift_remove(&key);
                Ok(Value::Map(entries))
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
                Ok(Value::list(entries.keys().map(MapKey::to_value).collect()))
            }
            IntrinsicFn::MapValues => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_values expects a Map".into()));
                };
                Ok(Value::list(entries.values().cloned().collect()))
            }
            IntrinsicFn::MapIsEmpty => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_is_empty expects a Map".into()));
                };
                Ok(Value::Bool(entries.is_empty()))
            }
            // ── Set (simple) ─────────────────────────────────────
            IntrinsicFn::SetNew => Ok(Value::set(IndexSet::new())),
            IntrinsicFn::SetInsert => {
                let mut args = args;
                let elem_value = args.pop().ok_or(RuntimeError::TypeError(
                    "set_insert: missing element argument".into(),
                ))?;
                let Value::Set(mut entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "set_insert: missing set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("set_insert expects a Set".into()));
                };
                let elem = MapKey::from_value(&elem_value)?;
                if entries.contains(&elem) {
                    return Ok(Value::Set(entries));
                }
                Rc::make_mut(&mut entries).insert(elem);
                Ok(Value::Set(entries))
            }
            IntrinsicFn::SetContains => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_contains expects a Set".into()));
                };
                let elem = MapKey::from_value(&args[1])?;
                Ok(Value::Bool(entries.contains(&elem)))
            }
            IntrinsicFn::SetRemove => {
                let mut args = args;
                let elem_value = args.pop().ok_or(RuntimeError::TypeError(
                    "set_remove: missing element argument".into(),
                ))?;
                let Value::Set(mut entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "set_remove: missing set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("set_remove expects a Set".into()));
                };
                let elem = MapKey::from_value(&elem_value)?;
                if !entries.contains(&elem) {
                    return Ok(Value::Set(entries));
                }
                Rc::make_mut(&mut entries).shift_remove(&elem);
                Ok(Value::Set(entries))
            }
            IntrinsicFn::SetLen => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_len expects a Set".into()));
                };
                Ok(Value::Int(entries.len() as i64))
            }
            IntrinsicFn::SetIsEmpty => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_is_empty expects a Set".into()));
                };
                Ok(Value::Bool(entries.is_empty()))
            }
            IntrinsicFn::SetValues => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_values expects a Set".into()));
                };
                Ok(Value::list(entries.iter().map(MapKey::to_value).collect()))
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
                Ok(Value::list(parts))
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
            IntrinsicFn::Gcd => {
                let Value::Int(a) = &args[0] else {
                    return Err(RuntimeError::TypeError("gcd expects Int arguments".into()));
                };
                let Value::Int(b) = &args[1] else {
                    return Err(RuntimeError::TypeError("gcd expects Int arguments".into()));
                };
                let g = gcd_u64(a.unsigned_abs(), b.unsigned_abs());
                let g = i64::try_from(g).map_err(|_| RuntimeError::IntegerOverflow)?;
                Ok(Value::Int(g))
            }
            IntrinsicFn::Lcm => {
                let Value::Int(a) = &args[0] else {
                    return Err(RuntimeError::TypeError("lcm expects Int arguments".into()));
                };
                let Value::Int(b) = &args[1] else {
                    return Err(RuntimeError::TypeError("lcm expects Int arguments".into()));
                };
                if *a == 0 || *b == 0 {
                    return Ok(Value::Int(0));
                }
                let abs_a = a.unsigned_abs();
                let abs_b = b.unsigned_abs();
                let g = gcd_u64(abs_a, abs_b);
                let lhs = abs_a / g;
                let lcm = u128::from(lhs)
                    .checked_mul(u128::from(abs_b))
                    .ok_or(RuntimeError::IntegerOverflow)?;
                if lcm > i64::MAX as u128 {
                    return Err(RuntimeError::IntegerOverflow);
                }
                Ok(Value::Int(lcm as i64))
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

            // ── Parsing (now complex — needs interpreter for Result construction) ──
            IntrinsicFn::ResultUnwrapOr
            | IntrinsicFn::ResultMapOr
            | IntrinsicFn::ParseInt
            | IntrinsicFn::ParseFloat => Err(RuntimeError::TypeError(
                "intrinsic needs interpreter context".into(),
            )),

            // ── String decomposition ──────────────────────────────
            IntrinsicFn::StringLines => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_lines expects a String argument".into(),
                    ));
                };
                let lines: Vec<Value> = s.lines().map(|l| Value::String(l.to_string())).collect();
                Ok(Value::list(lines))
            }
            IntrinsicFn::StringChars => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_chars expects a String argument".into(),
                    ));
                };
                let chars: Vec<Value> = s.chars().map(Value::Char).collect();
                Ok(Value::list(chars))
            }

            // ── File I/O ──────────────────────────────────────────
            IntrinsicFn::ReadFile => {
                let Value::String(path) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "read_file expects a String argument".into(),
                    ));
                };
                std::fs::read_to_string(path)
                    .map(Value::String)
                    .map_err(|e| RuntimeError::TypeError(format!("read_file: {e}")))
            }
            IntrinsicFn::ReadLine => {
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| RuntimeError::TypeError(format!("read_line: {e}")))?;
                // Trim trailing newline.
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Value::String(line))
            }
            IntrinsicFn::ReadStdin => {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| RuntimeError::TypeError(format!("read_stdin: {e}")))?;
                Ok(Value::String(buf))
            }

            // ── Sorting (natural order) ───────────────────────────
            IntrinsicFn::ListSort => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_sort expects a List".into()));
                };
                if xs.is_empty() {
                    return Ok(Value::list(Vec::new()));
                }
                match &xs[0] {
                    Value::Int(_) => {
                        let mut ints = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Int(n) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_sort: mixed types in list".into(),
                                ));
                            };
                            ints.push(*n);
                        }
                        ints.sort();
                        Ok(Value::list(ints.into_iter().map(Value::Int).collect()))
                    }
                    Value::Float(_) => {
                        let mut floats = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Float(f) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_sort: mixed types in list".into(),
                                ));
                            };
                            floats.push(*f);
                        }
                        floats.sort_by(f64::total_cmp);
                        Ok(Value::list(floats.into_iter().map(Value::Float).collect()))
                    }
                    Value::String(_) => {
                        let mut strings = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::String(s) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_sort: mixed types in list".into(),
                                ));
                            };
                            strings.push(s.clone());
                        }
                        strings.sort();
                        Ok(Value::list(
                            strings.into_iter().map(Value::String).collect(),
                        ))
                    }
                    Value::Char(_) => {
                        let mut chars = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Char(c) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_sort: mixed types in list".into(),
                                ));
                            };
                            chars.push(*c);
                        }
                        chars.sort();
                        Ok(Value::list(chars.into_iter().map(Value::Char).collect()))
                    }
                    Value::Bool(_) => {
                        let mut bools = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Bool(b) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_sort: mixed types in list".into(),
                                ));
                            };
                            bools.push(*b);
                        }
                        bools.sort();
                        Ok(Value::list(bools.into_iter().map(Value::Bool).collect()))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "list_sort: unsortable element type".into(),
                    )),
                }
            }
            IntrinsicFn::ListBinarySearch => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_binary_search expects a List".into(),
                    ));
                };
                let needle = &args[1];
                if xs.is_empty() {
                    return Ok(Value::Int(-1));
                }
                match &xs[0] {
                    Value::Int(_) => {
                        let Value::Int(needle) = needle else {
                            return Err(RuntimeError::TypeError(
                                "list_binary_search: mixed types in list".into(),
                            ));
                        };
                        let mut ints = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Int(n) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_binary_search: mixed types in list".into(),
                                ));
                            };
                            ints.push(*n);
                        }
                        encode_binary_search(ints.binary_search(needle))
                    }
                    Value::Float(_) => {
                        let Value::Float(needle) = needle else {
                            return Err(RuntimeError::TypeError(
                                "list_binary_search: mixed types in list".into(),
                            ));
                        };
                        let mut floats = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Float(f) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_binary_search: mixed types in list".into(),
                                ));
                            };
                            floats.push(*f);
                        }
                        encode_binary_search(floats.binary_search_by(|x| x.total_cmp(needle)))
                    }
                    Value::String(_) => {
                        let Value::String(needle) = needle else {
                            return Err(RuntimeError::TypeError(
                                "list_binary_search: mixed types in list".into(),
                            ));
                        };
                        let mut strings = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::String(s) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_binary_search: mixed types in list".into(),
                                ));
                            };
                            strings.push(s.clone());
                        }
                        encode_binary_search(strings.binary_search(needle))
                    }
                    Value::Char(_) => {
                        let Value::Char(needle) = needle else {
                            return Err(RuntimeError::TypeError(
                                "list_binary_search: mixed types in list".into(),
                            ));
                        };
                        let mut chars = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Char(c) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_binary_search: mixed types in list".into(),
                                ));
                            };
                            chars.push(*c);
                        }
                        encode_binary_search(chars.binary_search(needle))
                    }
                    Value::Bool(_) => {
                        let Value::Bool(needle) = needle else {
                            return Err(RuntimeError::TypeError(
                                "list_binary_search: mixed types in list".into(),
                            ));
                        };
                        let mut bools = Vec::with_capacity(xs.len());
                        for v in xs.iter() {
                            let Value::Bool(b) = v else {
                                return Err(RuntimeError::TypeError(
                                    "list_binary_search: mixed types in list".into(),
                                ));
                            };
                            bools.push(*b);
                        }
                        encode_binary_search(bools.binary_search(needle))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "list_binary_search: unsortable element type".into(),
                    )),
                }
            }

            // ── Complex (intercepted by interpreter) ─────────────
            IntrinsicFn::ListGet
            | IntrinsicFn::ListHead
            | IntrinsicFn::ListMap
            | IntrinsicFn::ListFilter
            | IntrinsicFn::ListFold
            | IntrinsicFn::ListEnumerate
            | IntrinsicFn::ListZip
            | IntrinsicFn::MapGet
            | IntrinsicFn::ListSortBy => Err(RuntimeError::TypeError(
                "complex intrinsic called without interpreter context".into(),
            )),
            // ReadLine and ReadStdin are handled above (they don't need interpreter context).
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
        (Name::new(interner, "list_range"), IntrinsicFn::ListRange),
        (
            Name::new(interner, "list_enumerate"),
            IntrinsicFn::ListEnumerate,
        ),
        (Name::new(interner, "list_zip"), IntrinsicFn::ListZip),
        (Name::new(interner, "list_chunks"), IntrinsicFn::ListChunks),
        (
            Name::new(interner, "list_windows"),
            IntrinsicFn::ListWindows,
        ),
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
        // Set
        (Name::new(interner, "set_new"), IntrinsicFn::SetNew),
        (Name::new(interner, "set_insert"), IntrinsicFn::SetInsert),
        (
            Name::new(interner, "set_contains"),
            IntrinsicFn::SetContains,
        ),
        (Name::new(interner, "set_remove"), IntrinsicFn::SetRemove),
        (Name::new(interner, "set_len"), IntrinsicFn::SetLen),
        (Name::new(interner, "set_is_empty"), IntrinsicFn::SetIsEmpty),
        (Name::new(interner, "set_values"), IntrinsicFn::SetValues),
        (
            Name::new(interner, "result_unwrap_or"),
            IntrinsicFn::ResultUnwrapOr,
        ),
        (Name::new(interner, "result_map_or"), IntrinsicFn::ResultMapOr),
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
        (Name::new(interner, "gcd"), IntrinsicFn::Gcd),
        (Name::new(interner, "lcm"), IntrinsicFn::Lcm),
        (Name::new(interner, "float_abs"), IntrinsicFn::FloatAbs),
        (Name::new(interner, "float_min"), IntrinsicFn::FloatMin),
        (Name::new(interner, "float_max"), IntrinsicFn::FloatMax),
        (Name::new(interner, "int_to_float"), IntrinsicFn::IntToFloat),
        (Name::new(interner, "float_to_int"), IntrinsicFn::FloatToInt),
        // Parsing
        (Name::new(interner, "parse_int"), IntrinsicFn::ParseInt),
        (Name::new(interner, "parse_float"), IntrinsicFn::ParseFloat),
        // String decomposition
        (
            Name::new(interner, "string_lines"),
            IntrinsicFn::StringLines,
        ),
        (
            Name::new(interner, "string_chars"),
            IntrinsicFn::StringChars,
        ),
        // File I/O
        (Name::new(interner, "read_file"), IntrinsicFn::ReadFile),
        (Name::new(interner, "read_line"), IntrinsicFn::ReadLine),
        (Name::new(interner, "read_stdin"), IntrinsicFn::ReadStdin),
        // Sorting
        (Name::new(interner, "list_sort"), IntrinsicFn::ListSort),
        (Name::new(interner, "list_sort_by"), IntrinsicFn::ListSortBy),
        (
            Name::new(interner, "list_binary_search"),
            IntrinsicFn::ListBinarySearch,
        ),
    ]
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn encode_binary_search(result: Result<usize, usize>) -> Result<Value, RuntimeError> {
    match result {
        Ok(idx) => {
            let idx = i64::try_from(idx).map_err(|_| RuntimeError::IntegerOverflow)?;
            Ok(Value::Int(idx))
        }
        Err(insertion_point) => {
            let insertion_point =
                i64::try_from(insertion_point).map_err(|_| RuntimeError::IntegerOverflow)?;
            let encoded = insertion_point
                .checked_add(1)
                .and_then(|n| n.checked_neg())
                .ok_or(RuntimeError::IntegerOverflow)?;
            Ok(Value::Int(encoded))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;
    use std::rc::Rc;

    use indexmap::IndexMap;
    use smallvec::smallvec;

    use super::*;

    #[test]
    fn list_push_detaches_when_storage_is_shared() {
        let original = Value::list(vec![Value::Int(1)]);
        let alias = original.clone();

        let pushed = IntrinsicFn::ListPush
            .call(smallvec![original, Value::Int(2)])
            .expect("list_push should succeed");

        let Value::List(alias_items) = &alias else {
            panic!("alias should remain a list");
        };
        let Value::List(pushed_items) = &pushed else {
            panic!("result should be a list");
        };

        assert_eq!(alias_items.len(), 1);
        assert_eq!(pushed_items.len(), 2);
        assert_eq!(alias_items[0], Value::Int(1));
        assert_eq!(pushed_items[1], Value::Int(2));
        assert!(
            !Rc::ptr_eq(alias_items, pushed_items),
            "mutation through list_push must detach shared storage"
        );
    }

    #[test]
    fn list_push_reuses_storage_when_not_shared() {
        let original = Value::list(vec![Value::Int(1)]);
        let before_ptr = match &original {
            Value::List(items) => Rc::as_ptr(items),
            _ => panic!("expected list"),
        };

        let pushed = IntrinsicFn::ListPush
            .call(smallvec![original, Value::Int(2)])
            .expect("list_push should succeed");

        let Value::List(pushed_items) = &pushed else {
            panic!("result should be a list");
        };

        assert_eq!(pushed_items.len(), 2);
        assert!(
            ptr::eq(before_ptr, Rc::as_ptr(pushed_items)),
            "list_push should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn map_insert_detaches_when_storage_is_shared() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(base);
        let alias = original.clone();

        let inserted = IntrinsicFn::MapInsert
            .call(smallvec![original, Value::Int(2), Value::Int(20)])
            .expect("map_insert should succeed");

        let Value::Map(alias_entries) = &alias else {
            panic!("alias should remain a map");
        };
        let Value::Map(inserted_entries) = &inserted else {
            panic!("result should be a map");
        };

        assert_eq!(alias_entries.len(), 1);
        assert_eq!(inserted_entries.len(), 2);
        assert_eq!(alias_entries.get(&MapKey::Int(2)), None);
        assert_eq!(inserted_entries.get(&MapKey::Int(2)), Some(&Value::Int(20)));
        assert!(
            !Rc::ptr_eq(alias_entries, inserted_entries),
            "mutation through map_insert must detach shared storage"
        );
    }

    #[test]
    fn map_insert_reuses_storage_when_not_shared() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(base);
        let before_ptr = match &original {
            Value::Map(entries) => Rc::as_ptr(entries),
            _ => panic!("expected map"),
        };

        let inserted = IntrinsicFn::MapInsert
            .call(smallvec![original, Value::Int(2), Value::Int(20)])
            .expect("map_insert should succeed");

        let Value::Map(inserted_entries) = &inserted else {
            panic!("result should be a map");
        };

        assert_eq!(inserted_entries.len(), 2);
        assert_eq!(inserted_entries.get(&MapKey::Int(2)), Some(&Value::Int(20)));
        assert!(
            ptr::eq(before_ptr, Rc::as_ptr(inserted_entries)),
            "map_insert should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn map_remove_detaches_when_storage_is_shared() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        base.insert(MapKey::Int(2), Value::Int(20));
        let original = Value::map(base);
        let alias = original.clone();

        let removed = IntrinsicFn::MapRemove
            .call(smallvec![original, Value::Int(2)])
            .expect("map_remove should succeed");

        let Value::Map(alias_entries) = &alias else {
            panic!("alias should remain a map");
        };
        let Value::Map(removed_entries) = &removed else {
            panic!("result should be a map");
        };

        assert_eq!(alias_entries.len(), 2);
        assert_eq!(removed_entries.len(), 1);
        assert_eq!(alias_entries.get(&MapKey::Int(2)), Some(&Value::Int(20)));
        assert_eq!(removed_entries.get(&MapKey::Int(2)), None);
        assert!(
            !Rc::ptr_eq(alias_entries, removed_entries),
            "mutation through map_remove must detach shared storage"
        );
    }

    #[test]
    fn map_remove_reuses_storage_when_not_shared() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        base.insert(MapKey::Int(2), Value::Int(20));
        let original = Value::map(base);
        let before_ptr = match &original {
            Value::Map(entries) => Rc::as_ptr(entries),
            _ => panic!("expected map"),
        };

        let removed = IntrinsicFn::MapRemove
            .call(smallvec![original, Value::Int(2)])
            .expect("map_remove should succeed");

        let Value::Map(removed_entries) = &removed else {
            panic!("result should be a map");
        };

        assert_eq!(removed_entries.len(), 1);
        assert_eq!(removed_entries.get(&MapKey::Int(2)), None);
        assert!(
            ptr::eq(before_ptr, Rc::as_ptr(removed_entries)),
            "map_remove should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn map_remove_missing_keeps_shared_storage() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(base);
        let alias = original.clone();

        let removed = IntrinsicFn::MapRemove
            .call(smallvec![original, Value::Int(99)])
            .expect("map_remove should succeed");

        let Value::Map(alias_entries) = &alias else {
            panic!("alias should remain a map");
        };
        let Value::Map(removed_entries) = &removed else {
            panic!("result should be a map");
        };

        assert_eq!(removed_entries.len(), 1);
        assert!(
            Rc::ptr_eq(alias_entries, removed_entries),
            "map_remove missing key should not detach shared storage"
        );
    }

    #[test]
    fn map_insert_same_value_keeps_shared_storage() {
        let mut base = IndexMap::new();
        base.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(base);
        let alias = original.clone();

        let inserted = IntrinsicFn::MapInsert
            .call(smallvec![original, Value::Int(1), Value::Int(10)])
            .expect("map_insert should succeed");

        let Value::Map(alias_entries) = &alias else {
            panic!("alias should remain a map");
        };
        let Value::Map(inserted_entries) = &inserted else {
            panic!("result should be a map");
        };

        assert_eq!(inserted_entries.len(), 1);
        assert_eq!(inserted_entries.get(&MapKey::Int(1)), Some(&Value::Int(10)));
        assert!(
            Rc::ptr_eq(alias_entries, inserted_entries),
            "map_insert with identical value should not detach shared storage"
        );
    }

    #[test]
    fn set_insert_detaches_when_storage_is_shared() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        let original = Value::set(base);
        let alias = original.clone();

        let inserted = IntrinsicFn::SetInsert
            .call(smallvec![original, Value::Int(2)])
            .expect("set_insert should succeed");

        let Value::Set(alias_entries) = &alias else {
            panic!("alias should remain a set");
        };
        let Value::Set(inserted_entries) = &inserted else {
            panic!("result should be a set");
        };

        assert_eq!(alias_entries.len(), 1);
        assert_eq!(inserted_entries.len(), 2);
        assert!(!alias_entries.contains(&MapKey::Int(2)));
        assert!(inserted_entries.contains(&MapKey::Int(2)));
        assert!(
            !Rc::ptr_eq(alias_entries, inserted_entries),
            "mutation through set_insert must detach shared storage"
        );
    }

    #[test]
    fn set_insert_reuses_storage_when_not_shared() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        let original = Value::set(base);
        let before_ptr = match &original {
            Value::Set(entries) => Rc::as_ptr(entries),
            _ => panic!("expected set"),
        };

        let inserted = IntrinsicFn::SetInsert
            .call(smallvec![original, Value::Int(2)])
            .expect("set_insert should succeed");

        let Value::Set(inserted_entries) = &inserted else {
            panic!("result should be a set");
        };

        assert_eq!(inserted_entries.len(), 2);
        assert!(inserted_entries.contains(&MapKey::Int(2)));
        assert!(
            ptr::eq(before_ptr, Rc::as_ptr(inserted_entries)),
            "set_insert should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn set_remove_detaches_when_storage_is_shared() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        base.insert(MapKey::Int(2));
        let original = Value::set(base);
        let alias = original.clone();

        let removed = IntrinsicFn::SetRemove
            .call(smallvec![original, Value::Int(2)])
            .expect("set_remove should succeed");

        let Value::Set(alias_entries) = &alias else {
            panic!("alias should remain a set");
        };
        let Value::Set(removed_entries) = &removed else {
            panic!("result should be a set");
        };

        assert_eq!(alias_entries.len(), 2);
        assert_eq!(removed_entries.len(), 1);
        assert!(alias_entries.contains(&MapKey::Int(2)));
        assert!(!removed_entries.contains(&MapKey::Int(2)));
        assert!(
            !Rc::ptr_eq(alias_entries, removed_entries),
            "mutation through set_remove must detach shared storage"
        );
    }

    #[test]
    fn set_remove_reuses_storage_when_not_shared() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        base.insert(MapKey::Int(2));
        let original = Value::set(base);
        let before_ptr = match &original {
            Value::Set(entries) => Rc::as_ptr(entries),
            _ => panic!("expected set"),
        };

        let removed = IntrinsicFn::SetRemove
            .call(smallvec![original, Value::Int(2)])
            .expect("set_remove should succeed");

        let Value::Set(removed_entries) = &removed else {
            panic!("result should be a set");
        };

        assert_eq!(removed_entries.len(), 1);
        assert!(!removed_entries.contains(&MapKey::Int(2)));
        assert!(
            ptr::eq(before_ptr, Rc::as_ptr(removed_entries)),
            "set_remove should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn set_insert_duplicate_keeps_shared_storage() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        let original = Value::set(base);
        let alias = original.clone();

        let inserted = IntrinsicFn::SetInsert
            .call(smallvec![original, Value::Int(1)])
            .expect("set_insert should succeed");

        let Value::Set(alias_entries) = &alias else {
            panic!("alias should remain a set");
        };
        let Value::Set(inserted_entries) = &inserted else {
            panic!("result should be a set");
        };

        assert_eq!(inserted_entries.len(), 1);
        assert!(
            Rc::ptr_eq(alias_entries, inserted_entries),
            "set_insert duplicate should not detach shared storage"
        );
    }

    #[test]
    fn set_remove_missing_keeps_shared_storage() {
        let mut base = IndexSet::new();
        base.insert(MapKey::Int(1));
        let original = Value::set(base);
        let alias = original.clone();

        let removed = IntrinsicFn::SetRemove
            .call(smallvec![original, Value::Int(99)])
            .expect("set_remove should succeed");

        let Value::Set(alias_entries) = &alias else {
            panic!("alias should remain a set");
        };
        let Value::Set(removed_entries) = &removed else {
            panic!("result should be a set");
        };

        assert_eq!(removed_entries.len(), 1);
        assert!(
            Rc::ptr_eq(alias_entries, removed_entries),
            "set_remove missing value should not detach shared storage"
        );
    }
}
