//! Built-in functions available to Kyokara programs.

use std::collections::VecDeque;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;
use md5::{Digest, Md5};
use smallvec::SmallVec;

use crate::error::RuntimeError;
use crate::value::{MapKey, PriorityQueueDirection, SeqSource, Value};

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
    ListSet,
    ListUpdate,
    BitSetNew,
    BitSetTest,
    BitSetSet,
    BitSetReset,
    BitSetFlip,
    BitSetCount,
    BitSetSize,
    BitSetIsEmpty,
    BitSetValues,
    BitSetUnion,
    BitSetIntersection,
    BitSetDifference,
    BitSetXor,
    MutableListNew,
    MutableListFromList,
    MutableListPush,
    MutableListInsert,
    MutableListLast,
    MutableListPop,
    MutableListExtend,
    MutableListLen,
    MutableListIsEmpty,
    MutableListGet,
    MutableListSet,
    MutableListDeleteAt,
    MutableListRemoveAt,
    MutableListUpdate,
    MutablePriorityQueueNewMin,
    MutablePriorityQueueNewMax,
    MutablePriorityQueuePush,
    MutablePriorityQueuePeek,
    MutablePriorityQueuePop,
    MutablePriorityQueueLen,
    MutablePriorityQueueIsEmpty,
    MutableMapNew,
    MutableMapWithCapacity,
    MutableMapInsert,
    MutableMapGet,
    MutableMapGetOrInsertWith,
    MutableMapContains,
    MutableMapRemove,
    MutableMapLen,
    MutableMapKeys,
    MutableMapValues,
    MutableMapIsEmpty,
    MutableSetNew,
    MutableSetWithCapacity,
    MutableSetInsert,
    MutableSetContains,
    MutableSetRemove,
    MutableSetLen,
    MutableSetIsEmpty,
    MutableSetValues,
    MutableBitSetNew,
    MutableBitSetTest,
    MutableBitSetSet,
    MutableBitSetReset,
    MutableBitSetFlip,
    MutableBitSetCount,
    MutableBitSetSize,
    MutableBitSetIsEmpty,
    MutableBitSetValues,
    MutableBitSetUnion,
    MutableBitSetIntersection,
    MutableBitSetDifference,
    MutableBitSetXor,
    DequeNew,
    DequePushFront,
    DequePushBack,
    DequePopFront,
    DequePopBack,
    DequeLen,
    DequeIsEmpty,
    SeqRange,
    SeqMap,
    SeqFilter,
    SeqFold,
    SeqScan,
    SeqUnfold,
    SeqEnumerate,
    SeqZip,
    SeqChunks,
    SeqWindows,
    SeqCount,
    SeqCountBy,
    SeqContains,
    SeqFrequencies,
    SeqAny,
    SeqAll,
    SeqFind,
    SeqToList,

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
    // Option<T>
    OptionUnwrapOr,
    OptionMapOr,
    OptionMap,
    OptionAndThen,
    // Result<T, E>
    ResultUnwrapOr,
    ResultMap,
    ResultAndThen,
    ResultMapErr,
    ResultMapOr,

    // String ops
    StringLen,
    StringContains,
    StringStartsWith,
    StringEndsWith,
    StringTrim,
    StringMd5,
    StringSplit,
    StringSubstring,
    StringToUpper,
    StringToLower,
    CharToString,
    CharCode,
    CharIsDecimalDigit,
    CharToDecimalDigit,
    CharToDigit,

    // Int/Float math
    Abs,
    IntPow,
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

pub(crate) fn char_digit_value(c: char, radix: i64) -> Result<Option<i64>, RuntimeError> {
    if !(2..=36).contains(&radix) {
        return Err(RuntimeError::TypeError(
            "char_to_digit: radix must be in 2..=36".into(),
        ));
    }

    let value = match c {
        '0'..='9' => Some(i64::from(c as u32 - '0' as u32)),
        'a'..='z' => Some(i64::from(c as u32 - 'a' as u32) + 10),
        'A'..='Z' => Some(i64::from(c as u32 - 'A' as u32) + 10),
        _ => None,
    };

    Ok(value.filter(|digit| *digit < radix))
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
                | IntrinsicFn::ListUpdate
                | IntrinsicFn::MutableListLast
                | IntrinsicFn::MutableListPop
                | IntrinsicFn::MutableListGet
                | IntrinsicFn::MutableListUpdate
                | IntrinsicFn::MutablePriorityQueuePush
                | IntrinsicFn::MutablePriorityQueuePeek
                | IntrinsicFn::MutablePriorityQueuePop
                | IntrinsicFn::MutableMapInsert
                | IntrinsicFn::MutableMapGet
                | IntrinsicFn::MutableMapGetOrInsertWith
                | IntrinsicFn::MutableMapContains
                | IntrinsicFn::MutableMapRemove
                | IntrinsicFn::MutableSetInsert
                | IntrinsicFn::MutableSetContains
                | IntrinsicFn::MutableSetRemove
                | IntrinsicFn::DequePopFront
                | IntrinsicFn::DequePopBack
                | IntrinsicFn::SeqMap
                | IntrinsicFn::SeqFilter
                | IntrinsicFn::SeqFold
                | IntrinsicFn::SeqScan
                | IntrinsicFn::SeqUnfold
                | IntrinsicFn::SeqEnumerate
                | IntrinsicFn::SeqZip
                | IntrinsicFn::SeqChunks
                | IntrinsicFn::SeqWindows
                | IntrinsicFn::SeqCount
                | IntrinsicFn::SeqCountBy
                | IntrinsicFn::SeqContains
                | IntrinsicFn::SeqFrequencies
                | IntrinsicFn::SeqAny
                | IntrinsicFn::SeqAll
                | IntrinsicFn::SeqFind
                | IntrinsicFn::SeqToList
                | IntrinsicFn::MapInsert
                | IntrinsicFn::MapContains
                | IntrinsicFn::MapGet
                | IntrinsicFn::MapRemove
                | IntrinsicFn::SetInsert
                | IntrinsicFn::SetContains
                | IntrinsicFn::SetRemove
                | IntrinsicFn::ListSort
                | IntrinsicFn::ListSortBy
                | IntrinsicFn::ListBinarySearch
                | IntrinsicFn::OptionUnwrapOr
                | IntrinsicFn::OptionMapOr
                | IntrinsicFn::OptionMap
                | IntrinsicFn::OptionAndThen
                | IntrinsicFn::ResultUnwrapOr
                | IntrinsicFn::ResultMap
                | IntrinsicFn::ResultAndThen
                | IntrinsicFn::ResultMapErr
                | IntrinsicFn::ResultMapOr
                | IntrinsicFn::ParseInt
                | IntrinsicFn::ParseFloat
                | IntrinsicFn::CharToDecimalDigit
                | IntrinsicFn::CharToDigit
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
            IntrinsicFn::ListSet => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "list_set: missing value argument".into(),
                ))?;
                let Value::Int(i) = args.pop().ok_or(RuntimeError::TypeError(
                    "list_set: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "list_set expects an Int index".into(),
                    ));
                };
                let Value::List(mut xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "list_set: missing list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("list_set expects a List".into()));
                };

                if i < 0 || i as usize >= xs.len() {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: xs.len() as i64,
                    });
                }

                Rc::make_mut(&mut xs)[i as usize] = val;
                Ok(Value::List(xs))
            }
            IntrinsicFn::BitSetNew => {
                let Value::Int(size) = &args[0] else {
                    return Err(RuntimeError::TypeError("bitset_new expects an Int".into()));
                };
                let size = usize::try_from(*size)
                    .map_err(|_| RuntimeError::TypeError("bitset size must be >= 0".into()))?;
                Ok(Value::bitset(size))
            }
            IntrinsicFn::BitSetTest => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_test expects a BitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_test expects an Int index".into(),
                    ));
                };
                Ok(Value::Bool(bitset.test(*idx)?))
            }
            IntrinsicFn::BitSetSet => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_set expects a BitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_set expects an Int index".into(),
                    ));
                };
                Ok(Value::BitSet(bitset.set(*idx)?))
            }
            IntrinsicFn::BitSetReset => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_reset expects a BitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_reset expects an Int index".into(),
                    ));
                };
                Ok(Value::BitSet(bitset.reset(*idx)?))
            }
            IntrinsicFn::BitSetFlip => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_flip expects a BitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_flip expects an Int index".into(),
                    ));
                };
                Ok(Value::BitSet(bitset.flip(*idx)?))
            }
            IntrinsicFn::BitSetCount => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_count expects a BitSet".into(),
                    ));
                };
                Ok(Value::Int(bitset.count() as i64))
            }
            IntrinsicFn::BitSetSize => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_size expects a BitSet".into(),
                    ));
                };
                Ok(Value::Int(bitset.size_bits() as i64))
            }
            IntrinsicFn::BitSetIsEmpty => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_is_empty expects a BitSet".into(),
                    ));
                };
                Ok(Value::Bool(bitset.is_empty()))
            }
            IntrinsicFn::BitSetValues => {
                let Value::BitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_values expects a BitSet".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::BitSetValues(bitset.clone())))
            }
            IntrinsicFn::BitSetUnion => {
                let Value::BitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_union expects BitSet arguments".into(),
                    ));
                };
                let Value::BitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_union expects BitSet arguments".into(),
                    ));
                };
                Ok(Value::BitSet(lhs.union(rhs)?))
            }
            IntrinsicFn::BitSetIntersection => {
                let Value::BitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_intersection expects BitSet arguments".into(),
                    ));
                };
                let Value::BitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_intersection expects BitSet arguments".into(),
                    ));
                };
                Ok(Value::BitSet(lhs.intersection(rhs)?))
            }
            IntrinsicFn::BitSetDifference => {
                let Value::BitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_difference expects BitSet arguments".into(),
                    ));
                };
                let Value::BitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_difference expects BitSet arguments".into(),
                    ));
                };
                Ok(Value::BitSet(lhs.difference(rhs)?))
            }
            IntrinsicFn::BitSetXor => {
                let Value::BitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_xor expects BitSet arguments".into(),
                    ));
                };
                let Value::BitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "bitset_xor expects BitSet arguments".into(),
                    ));
                };
                Ok(Value::BitSet(lhs.xor(rhs)?))
            }
            IntrinsicFn::MutableListNew => Ok(Value::mutable_list(Vec::new())),
            IntrinsicFn::MutableListFromList => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_from_list expects a List".into(),
                    ));
                };
                Ok(Value::mutable_list(xs.as_ref().clone()))
            }
            IntrinsicFn::MutableListPush => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_push: missing value argument".into(),
                ))?;
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_push: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_push expects a MutableList".into(),
                    ));
                };
                xs.push(val);
                Ok(Value::MutableList(xs))
            }
            IntrinsicFn::MutableListInsert => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_insert: missing value argument".into(),
                ))?;
                let Value::Int(i) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_insert: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_insert expects an Int index".into(),
                    ));
                };
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_insert: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_insert expects a MutableList".into(),
                    ));
                };

                let len = xs.len();
                if i < 0 || i as usize > len {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: len as i64,
                    });
                }

                xs.insert(i as usize, val);
                Ok(Value::MutableList(xs))
            }
            IntrinsicFn::MutableListExtend => {
                let mut args = args;
                let Value::List(values) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_extend: missing values argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_extend expects a List".into(),
                    ));
                };
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_extend: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_extend expects a MutableList".into(),
                    ));
                };
                xs.extend(values.iter().cloned());
                Ok(Value::MutableList(xs))
            }
            IntrinsicFn::MutableListLen => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_len expects a MutableList".into(),
                    ));
                };
                Ok(Value::Int(xs.len() as i64))
            }
            IntrinsicFn::MutableListIsEmpty => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_is_empty expects a MutableList".into(),
                    ));
                };
                Ok(Value::Bool(xs.is_empty()))
            }
            IntrinsicFn::MutableListSet => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_set: missing value argument".into(),
                ))?;
                let Value::Int(i) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_set: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_set expects an Int index".into(),
                    ));
                };
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_set: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_set expects a MutableList".into(),
                    ));
                };

                let len = xs.len();
                if i < 0 || i as usize >= len {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: len as i64,
                    });
                }

                xs.set(i as usize, val);
                Ok(Value::MutableList(xs))
            }
            IntrinsicFn::MutableListDeleteAt => {
                let mut args = args;
                let Value::Int(i) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_delete_at: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_delete_at expects an Int index".into(),
                    ));
                };
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_delete_at: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_delete_at expects a MutableList".into(),
                    ));
                };

                let len = xs.len();
                if i < 0 || i as usize >= len {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: len as i64,
                    });
                }

                xs.delete_at(i as usize);
                Ok(Value::MutableList(xs))
            }
            IntrinsicFn::MutableListRemoveAt => {
                let mut args = args;
                let Value::Int(i) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_remove_at: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_remove_at expects an Int index".into(),
                    ));
                };
                let Value::MutableList(xs) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_list_remove_at: missing mutable list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_remove_at expects a MutableList".into(),
                    ));
                };

                let len = xs.len();
                if i < 0 || i as usize >= len {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: len as i64,
                    });
                }

                Ok(xs.remove_at(i as usize))
            }
            IntrinsicFn::MutablePriorityQueueNewMin => {
                Ok(Value::mutable_priority_queue(PriorityQueueDirection::Min))
            }
            IntrinsicFn::MutablePriorityQueueNewMax => {
                Ok(Value::mutable_priority_queue(PriorityQueueDirection::Max))
            }
            IntrinsicFn::MutablePriorityQueueLen => {
                let Value::MutablePriorityQueue(queue) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_priority_queue_len expects a MutablePriorityQueue".into(),
                    ));
                };
                Ok(Value::Int(queue.borrow().entries.len() as i64))
            }
            IntrinsicFn::MutablePriorityQueueIsEmpty => {
                let Value::MutablePriorityQueue(queue) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_priority_queue_is_empty expects a MutablePriorityQueue".into(),
                    ));
                };
                Ok(Value::Bool(queue.borrow().entries.is_empty()))
            }
            IntrinsicFn::MutableMapNew => Ok(Value::mutable_map(IndexMap::new())),
            IntrinsicFn::MutableMapWithCapacity => {
                let Value::Int(capacity) = args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_with_capacity expects an Int".into(),
                    ));
                };
                let capacity = usize::try_from(capacity).map_err(|_| {
                    RuntimeError::TypeError(
                        "mutable_map_with_capacity: capacity must be >= 0".into(),
                    )
                })?;
                Ok(Value::mutable_map_with_capacity(capacity))
            }
            IntrinsicFn::MutableMapInsert => {
                let mut args = args;
                let value = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_map_insert: missing value argument".into(),
                ))?;
                let key_value = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_map_insert: missing key argument".into(),
                ))?;
                let Value::MutableMap(entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_map_insert: missing mutable map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_insert expects a MutableMap".into(),
                    ));
                };
                let key = MapKey::from_value(&key_value)?;
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                entries
                    .borrow_mut()
                    .insert_with(hash, key_value, value, &mut |lhs, rhs| Ok(lhs == rhs))?;
                Ok(Value::MutableMap(entries))
            }
            IntrinsicFn::MutableMapContains => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_contains expects a MutableMap".into(),
                    ));
                };
                let key = MapKey::from_value(&args[1])?;
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                Ok(Value::Bool(entries.borrow().contains_with(
                    hash,
                    &key_value,
                    &mut |lhs, rhs| Ok(lhs == rhs),
                )?))
            }
            IntrinsicFn::MutableMapRemove => {
                let mut args = args;
                let key_value = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_map_remove: missing key argument".into(),
                ))?;
                let Value::MutableMap(entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_map_remove: missing mutable map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_remove expects a MutableMap".into(),
                    ));
                };
                let key = MapKey::from_value(&key_value)?;
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                entries
                    .borrow_mut()
                    .remove_with(hash, &key_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
                Ok(Value::MutableMap(entries))
            }
            IntrinsicFn::MutableMapLen => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_len expects a MutableMap".into(),
                    ));
                };
                Ok(Value::Int(entries.borrow().len() as i64))
            }
            IntrinsicFn::MutableMapKeys => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_keys expects a MutableMap".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::MapKeys(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::MutableMapValues => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_values expects a MutableMap".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::MapValues(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::MutableMapIsEmpty => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_is_empty expects a MutableMap".into(),
                    ));
                };
                Ok(Value::Bool(entries.borrow().is_empty()))
            }
            IntrinsicFn::MutableSetNew => Ok(Value::mutable_set(IndexSet::new())),
            IntrinsicFn::MutableSetWithCapacity => {
                let Value::Int(capacity) = args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_with_capacity expects an Int".into(),
                    ));
                };
                let capacity = usize::try_from(capacity).map_err(|_| {
                    RuntimeError::TypeError(
                        "mutable_set_with_capacity: capacity must be >= 0".into(),
                    )
                })?;
                Ok(Value::mutable_set_with_capacity(capacity))
            }
            IntrinsicFn::MutableSetInsert => {
                let mut args = args;
                let elem_value = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_set_insert: missing element argument".into(),
                ))?;
                let Value::MutableSet(entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_set_insert: missing mutable set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_insert expects a MutableSet".into(),
                    ));
                };
                let elem = MapKey::from_value(&elem_value)?;
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                entries
                    .borrow_mut()
                    .insert_with(hash, elem_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
                Ok(Value::MutableSet(entries))
            }
            IntrinsicFn::MutableSetContains => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_contains expects a MutableSet".into(),
                    ));
                };
                let elem = MapKey::from_value(&args[1])?;
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                Ok(Value::Bool(entries.borrow().contains_with(
                    hash,
                    &elem_value,
                    &mut |lhs, rhs| Ok(lhs == rhs),
                )?))
            }
            IntrinsicFn::MutableSetRemove => {
                let mut args = args;
                let elem_value = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_set_remove: missing element argument".into(),
                ))?;
                let Value::MutableSet(entries) = args.pop().ok_or(RuntimeError::TypeError(
                    "mutable_set_remove: missing mutable set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_remove expects a MutableSet".into(),
                    ));
                };
                let elem = MapKey::from_value(&elem_value)?;
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                entries
                    .borrow_mut()
                    .remove_with(hash, &elem_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
                Ok(Value::MutableSet(entries))
            }
            IntrinsicFn::MutableSetLen => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_len expects a MutableSet".into(),
                    ));
                };
                Ok(Value::Int(entries.borrow().len() as i64))
            }
            IntrinsicFn::MutableSetIsEmpty => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_is_empty expects a MutableSet".into(),
                    ));
                };
                Ok(Value::Bool(entries.borrow().is_empty()))
            }
            IntrinsicFn::MutableSetValues => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_values expects a MutableSet".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::SetValues(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::MutableBitSetNew => {
                let Value::Int(size) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_new expects an Int".into(),
                    ));
                };
                let size = usize::try_from(*size)
                    .map_err(|_| RuntimeError::TypeError("bitset size must be >= 0".into()))?;
                Ok(Value::mutable_bitset(size))
            }
            IntrinsicFn::MutableBitSetTest => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_test expects a MutableBitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_test expects an Int index".into(),
                    ));
                };
                Ok(Value::Bool(bitset.test(*idx)?))
            }
            IntrinsicFn::MutableBitSetSet => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_set expects a MutableBitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_set expects an Int index".into(),
                    ));
                };
                bitset.set(*idx)?;
                Ok(Value::MutableBitSet(bitset.clone()))
            }
            IntrinsicFn::MutableBitSetReset => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_reset expects a MutableBitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_reset expects an Int index".into(),
                    ));
                };
                bitset.reset(*idx)?;
                Ok(Value::MutableBitSet(bitset.clone()))
            }
            IntrinsicFn::MutableBitSetFlip => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_flip expects a MutableBitSet".into(),
                    ));
                };
                let Value::Int(idx) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_flip expects an Int index".into(),
                    ));
                };
                bitset.flip(*idx)?;
                Ok(Value::MutableBitSet(bitset.clone()))
            }
            IntrinsicFn::MutableBitSetCount => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_count expects a MutableBitSet".into(),
                    ));
                };
                Ok(Value::Int(bitset.count() as i64))
            }
            IntrinsicFn::MutableBitSetSize => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_size expects a MutableBitSet".into(),
                    ));
                };
                Ok(Value::Int(bitset.size_bits() as i64))
            }
            IntrinsicFn::MutableBitSetIsEmpty => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_is_empty expects a MutableBitSet".into(),
                    ));
                };
                Ok(Value::Bool(bitset.is_empty()))
            }
            IntrinsicFn::MutableBitSetValues => {
                let Value::MutableBitSet(bitset) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_values expects a MutableBitSet".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::BitSetValues(
                    bitset.snapshot(),
                )))
            }
            IntrinsicFn::MutableBitSetUnion => {
                let Value::MutableBitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_union expects MutableBitSet arguments".into(),
                    ));
                };
                let Value::MutableBitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_union expects MutableBitSet arguments".into(),
                    ));
                };
                lhs.union_assign(rhs)?;
                Ok(Value::MutableBitSet(lhs.clone()))
            }
            IntrinsicFn::MutableBitSetIntersection => {
                let Value::MutableBitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_intersection expects MutableBitSet arguments".into(),
                    ));
                };
                let Value::MutableBitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_intersection expects MutableBitSet arguments".into(),
                    ));
                };
                lhs.intersection_assign(rhs)?;
                Ok(Value::MutableBitSet(lhs.clone()))
            }
            IntrinsicFn::MutableBitSetDifference => {
                let Value::MutableBitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_difference expects MutableBitSet arguments".into(),
                    ));
                };
                let Value::MutableBitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_difference expects MutableBitSet arguments".into(),
                    ));
                };
                lhs.difference_assign(rhs)?;
                Ok(Value::MutableBitSet(lhs.clone()))
            }
            IntrinsicFn::MutableBitSetXor => {
                let Value::MutableBitSet(lhs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_xor expects MutableBitSet arguments".into(),
                    ));
                };
                let Value::MutableBitSet(rhs) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_bitset_xor expects MutableBitSet arguments".into(),
                    ));
                };
                lhs.xor_assign(rhs)?;
                Ok(Value::MutableBitSet(lhs.clone()))
            }
            IntrinsicFn::DequeNew => Ok(Value::deque(VecDeque::new())),
            IntrinsicFn::DequePushFront => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "deque_push_front: missing value argument".into(),
                ))?;
                let Value::Deque(mut q) = args.pop().ok_or(RuntimeError::TypeError(
                    "deque_push_front: missing deque argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "deque_push_front expects a Deque".into(),
                    ));
                };
                Rc::make_mut(&mut q).push_front(val);
                Ok(Value::Deque(q))
            }
            IntrinsicFn::DequePushBack => {
                let mut args = args;
                let val = args.pop().ok_or(RuntimeError::TypeError(
                    "deque_push_back: missing value argument".into(),
                ))?;
                let Value::Deque(mut q) = args.pop().ok_or(RuntimeError::TypeError(
                    "deque_push_back: missing deque argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "deque_push_back expects a Deque".into(),
                    ));
                };
                Rc::make_mut(&mut q).push_back(val);
                Ok(Value::Deque(q))
            }
            IntrinsicFn::DequeLen => {
                let Value::Deque(q) = &args[0] else {
                    return Err(RuntimeError::TypeError("deque_len expects a Deque".into()));
                };
                Ok(Value::Int(q.len() as i64))
            }
            IntrinsicFn::DequeIsEmpty => {
                let Value::Deque(q) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "deque_is_empty expects a Deque".into(),
                    ));
                };
                Ok(Value::Bool(q.is_empty()))
            }
            IntrinsicFn::SeqRange => {
                let Value::Int(start) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "seq_range expects Int arguments".into(),
                    ));
                };
                let Value::Int(end) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "seq_range expects Int arguments".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::Range {
                    start: *start,
                    end: *end,
                }))
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
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                if entries.get(&key) == Some(&value) {
                    return Ok(Value::Map(entries));
                }
                Rc::make_mut(&mut entries).insert_with(
                    hash,
                    key_value,
                    value,
                    &mut |lhs, rhs| Ok(lhs == rhs),
                )?;
                Ok(Value::Map(entries))
            }
            IntrinsicFn::MapContains => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_contains expects a Map".into()));
                };
                let key = MapKey::from_value(&args[1])?;
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                Ok(Value::Bool(entries.contains_with(
                    hash,
                    &key_value,
                    &mut |lhs, rhs| Ok(lhs == rhs),
                )?))
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
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                if entries.get(&key).is_none() {
                    return Ok(Value::Map(entries));
                }
                Rc::make_mut(&mut entries)
                    .remove_with(hash, &key_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
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
                Ok(Value::seq_source(SeqSource::MapKeys(entries.clone())))
            }
            IntrinsicFn::MapValues => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_values expects a Map".into()));
                };
                Ok(Value::seq_source(SeqSource::MapValues(entries.clone())))
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
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                if entries.contains(&elem) {
                    return Ok(Value::Set(entries));
                }
                Rc::make_mut(&mut entries)
                    .insert_with(hash, elem_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
                Ok(Value::Set(entries))
            }
            IntrinsicFn::SetContains => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_contains expects a Set".into()));
                };
                let elem = MapKey::from_value(&args[1])?;
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                Ok(Value::Bool(entries.contains_with(
                    hash,
                    &elem_value,
                    &mut |lhs, rhs| Ok(lhs == rhs),
                )?))
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
                let hash = elem.primitive_hash();
                let elem_value = elem.to_value();
                if !entries.contains(&elem) {
                    return Ok(Value::Set(entries));
                }
                Rc::make_mut(&mut entries)
                    .remove_with(hash, &elem_value, &mut |lhs, rhs| Ok(lhs == rhs))?;
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
                Ok(Value::seq_source(SeqSource::SetValues(entries.clone())))
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
            IntrinsicFn::StringMd5 => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_md5 expects a String".into(),
                    ));
                };
                Ok(Value::String(format!("{:x}", Md5::digest(s.as_bytes()))))
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
                Ok(Value::seq_source(SeqSource::StringSplit {
                    s: Rc::new(s.clone()),
                    delim: Rc::new(delim.clone()),
                }))
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
            IntrinsicFn::CharCode => {
                let Value::Char(c) = &args[0] else {
                    return Err(RuntimeError::TypeError("char_code expects a Char".into()));
                };
                Ok(Value::Int(i64::from(u32::from(*c))))
            }
            IntrinsicFn::CharIsDecimalDigit => {
                let Value::Char(c) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "char_is_decimal_digit expects a Char".into(),
                    ));
                };
                Ok(Value::Bool(char_digit_value(*c, 10)?.is_some()))
            }
            IntrinsicFn::CharToDecimalDigit | IntrinsicFn::CharToDigit => {
                Err(RuntimeError::TypeError(
                    "char digit conversion intrinsic requires interpreter".into(),
                ))
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
            IntrinsicFn::IntPow => {
                let Value::Int(base) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "int_pow expects Int arguments".into(),
                    ));
                };
                let Value::Int(exp) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "int_pow expects Int arguments".into(),
                    ));
                };
                if *exp < 0 {
                    return Err(RuntimeError::TypeError(
                        "int_pow: exponent must be >= 0".into(),
                    ));
                }

                let mut result: i64 = 1;
                let mut factor = *base;
                let mut power = u64::try_from(*exp).map_err(|_| {
                    RuntimeError::TypeError("int_pow: exponent must be >= 0".into())
                })?;
                while power > 0 {
                    if power & 1 == 1 {
                        result = result
                            .checked_mul(factor)
                            .ok_or(RuntimeError::IntegerOverflow)?;
                    }
                    power >>= 1;
                    if power > 0 {
                        factor = factor
                            .checked_mul(factor)
                            .ok_or(RuntimeError::IntegerOverflow)?;
                    }
                }

                Ok(Value::Int(result))
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
            IntrinsicFn::OptionUnwrapOr
            | IntrinsicFn::OptionMapOr
            | IntrinsicFn::OptionMap
            | IntrinsicFn::OptionAndThen
            | IntrinsicFn::ResultUnwrapOr
            | IntrinsicFn::ResultMap
            | IntrinsicFn::ResultAndThen
            | IntrinsicFn::ResultMapErr
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
                Ok(Value::seq_source(SeqSource::StringLines {
                    s: Rc::new(s.clone()),
                }))
            }
            IntrinsicFn::StringChars => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "string_chars expects a String argument".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::StringChars {
                    s: Rc::new(s.clone()),
                }))
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
            | IntrinsicFn::MutableListLast
            | IntrinsicFn::MutableListPop
            | IntrinsicFn::MutableListGet
            | IntrinsicFn::MutablePriorityQueuePush
            | IntrinsicFn::MutablePriorityQueuePeek
            | IntrinsicFn::MutablePriorityQueuePop
            | IntrinsicFn::MutableMapGet
            | IntrinsicFn::MutableMapGetOrInsertWith
            | IntrinsicFn::SeqMap
            | IntrinsicFn::SeqFilter
            | IntrinsicFn::SeqFold
            | IntrinsicFn::SeqScan
            | IntrinsicFn::SeqUnfold
            | IntrinsicFn::SeqEnumerate
            | IntrinsicFn::SeqZip
            | IntrinsicFn::SeqChunks
            | IntrinsicFn::SeqWindows
            | IntrinsicFn::SeqCount
            | IntrinsicFn::SeqCountBy
            | IntrinsicFn::SeqContains
            | IntrinsicFn::SeqFrequencies
            | IntrinsicFn::SeqAny
            | IntrinsicFn::SeqAll
            | IntrinsicFn::SeqFind
            | IntrinsicFn::SeqToList
            | IntrinsicFn::MapGet
            | IntrinsicFn::ListUpdate
            | IntrinsicFn::MutableListUpdate
            | IntrinsicFn::DequePopFront
            | IntrinsicFn::DequePopBack
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
        (Name::new(interner, "list_set"), IntrinsicFn::ListSet),
        (Name::new(interner, "list_update"), IntrinsicFn::ListUpdate),
        (Name::new(interner, "bitset_new"), IntrinsicFn::BitSetNew),
        (Name::new(interner, "bitset_test"), IntrinsicFn::BitSetTest),
        (Name::new(interner, "bitset_set"), IntrinsicFn::BitSetSet),
        (
            Name::new(interner, "bitset_reset"),
            IntrinsicFn::BitSetReset,
        ),
        (Name::new(interner, "bitset_flip"), IntrinsicFn::BitSetFlip),
        (
            Name::new(interner, "bitset_count"),
            IntrinsicFn::BitSetCount,
        ),
        (Name::new(interner, "bitset_size"), IntrinsicFn::BitSetSize),
        (
            Name::new(interner, "bitset_is_empty"),
            IntrinsicFn::BitSetIsEmpty,
        ),
        (
            Name::new(interner, "bitset_values"),
            IntrinsicFn::BitSetValues,
        ),
        (
            Name::new(interner, "bitset_union"),
            IntrinsicFn::BitSetUnion,
        ),
        (
            Name::new(interner, "bitset_intersection"),
            IntrinsicFn::BitSetIntersection,
        ),
        (
            Name::new(interner, "bitset_difference"),
            IntrinsicFn::BitSetDifference,
        ),
        (Name::new(interner, "bitset_xor"), IntrinsicFn::BitSetXor),
        (
            Name::new(interner, "mutable_list_new"),
            IntrinsicFn::MutableListNew,
        ),
        (
            Name::new(interner, "mutable_list_from_list"),
            IntrinsicFn::MutableListFromList,
        ),
        (
            Name::new(interner, "mutable_list_push"),
            IntrinsicFn::MutableListPush,
        ),
        (
            Name::new(interner, "mutable_list_insert"),
            IntrinsicFn::MutableListInsert,
        ),
        (
            Name::new(interner, "mutable_list_last"),
            IntrinsicFn::MutableListLast,
        ),
        (
            Name::new(interner, "mutable_list_pop"),
            IntrinsicFn::MutableListPop,
        ),
        (
            Name::new(interner, "mutable_list_extend"),
            IntrinsicFn::MutableListExtend,
        ),
        (
            Name::new(interner, "mutable_list_len"),
            IntrinsicFn::MutableListLen,
        ),
        (
            Name::new(interner, "mutable_list_is_empty"),
            IntrinsicFn::MutableListIsEmpty,
        ),
        (
            Name::new(interner, "mutable_list_get"),
            IntrinsicFn::MutableListGet,
        ),
        (
            Name::new(interner, "mutable_list_set"),
            IntrinsicFn::MutableListSet,
        ),
        (
            Name::new(interner, "mutable_list_delete_at"),
            IntrinsicFn::MutableListDeleteAt,
        ),
        (
            Name::new(interner, "mutable_list_remove_at"),
            IntrinsicFn::MutableListRemoveAt,
        ),
        (
            Name::new(interner, "mutable_list_update"),
            IntrinsicFn::MutableListUpdate,
        ),
        (
            Name::new(interner, "mutable_priority_queue_new_min"),
            IntrinsicFn::MutablePriorityQueueNewMin,
        ),
        (
            Name::new(interner, "mutable_priority_queue_new_max"),
            IntrinsicFn::MutablePriorityQueueNewMax,
        ),
        (
            Name::new(interner, "mutable_priority_queue_push"),
            IntrinsicFn::MutablePriorityQueuePush,
        ),
        (
            Name::new(interner, "mutable_priority_queue_peek"),
            IntrinsicFn::MutablePriorityQueuePeek,
        ),
        (
            Name::new(interner, "mutable_priority_queue_pop"),
            IntrinsicFn::MutablePriorityQueuePop,
        ),
        (
            Name::new(interner, "mutable_priority_queue_len"),
            IntrinsicFn::MutablePriorityQueueLen,
        ),
        (
            Name::new(interner, "mutable_priority_queue_is_empty"),
            IntrinsicFn::MutablePriorityQueueIsEmpty,
        ),
        (
            Name::new(interner, "mutable_map_new"),
            IntrinsicFn::MutableMapNew,
        ),
        (
            Name::new(interner, "mutable_map_with_capacity"),
            IntrinsicFn::MutableMapWithCapacity,
        ),
        (
            Name::new(interner, "mutable_map_insert"),
            IntrinsicFn::MutableMapInsert,
        ),
        (
            Name::new(interner, "mutable_map_get"),
            IntrinsicFn::MutableMapGet,
        ),
        (
            Name::new(interner, "mutable_map_get_or_insert_with"),
            IntrinsicFn::MutableMapGetOrInsertWith,
        ),
        (
            Name::new(interner, "mutable_map_contains"),
            IntrinsicFn::MutableMapContains,
        ),
        (
            Name::new(interner, "mutable_map_remove"),
            IntrinsicFn::MutableMapRemove,
        ),
        (
            Name::new(interner, "mutable_map_len"),
            IntrinsicFn::MutableMapLen,
        ),
        (
            Name::new(interner, "mutable_map_keys"),
            IntrinsicFn::MutableMapKeys,
        ),
        (
            Name::new(interner, "mutable_map_values"),
            IntrinsicFn::MutableMapValues,
        ),
        (
            Name::new(interner, "mutable_map_is_empty"),
            IntrinsicFn::MutableMapIsEmpty,
        ),
        (
            Name::new(interner, "mutable_set_new"),
            IntrinsicFn::MutableSetNew,
        ),
        (
            Name::new(interner, "mutable_set_with_capacity"),
            IntrinsicFn::MutableSetWithCapacity,
        ),
        (
            Name::new(interner, "mutable_set_insert"),
            IntrinsicFn::MutableSetInsert,
        ),
        (
            Name::new(interner, "mutable_set_contains"),
            IntrinsicFn::MutableSetContains,
        ),
        (
            Name::new(interner, "mutable_set_remove"),
            IntrinsicFn::MutableSetRemove,
        ),
        (
            Name::new(interner, "mutable_set_len"),
            IntrinsicFn::MutableSetLen,
        ),
        (
            Name::new(interner, "mutable_set_is_empty"),
            IntrinsicFn::MutableSetIsEmpty,
        ),
        (
            Name::new(interner, "mutable_set_values"),
            IntrinsicFn::MutableSetValues,
        ),
        (
            Name::new(interner, "mutable_bitset_new"),
            IntrinsicFn::MutableBitSetNew,
        ),
        (
            Name::new(interner, "mutable_bitset_test"),
            IntrinsicFn::MutableBitSetTest,
        ),
        (
            Name::new(interner, "mutable_bitset_set"),
            IntrinsicFn::MutableBitSetSet,
        ),
        (
            Name::new(interner, "mutable_bitset_reset"),
            IntrinsicFn::MutableBitSetReset,
        ),
        (
            Name::new(interner, "mutable_bitset_flip"),
            IntrinsicFn::MutableBitSetFlip,
        ),
        (
            Name::new(interner, "mutable_bitset_count"),
            IntrinsicFn::MutableBitSetCount,
        ),
        (
            Name::new(interner, "mutable_bitset_size"),
            IntrinsicFn::MutableBitSetSize,
        ),
        (
            Name::new(interner, "mutable_bitset_is_empty"),
            IntrinsicFn::MutableBitSetIsEmpty,
        ),
        (
            Name::new(interner, "mutable_bitset_values"),
            IntrinsicFn::MutableBitSetValues,
        ),
        (
            Name::new(interner, "mutable_bitset_union"),
            IntrinsicFn::MutableBitSetUnion,
        ),
        (
            Name::new(interner, "mutable_bitset_intersection"),
            IntrinsicFn::MutableBitSetIntersection,
        ),
        (
            Name::new(interner, "mutable_bitset_difference"),
            IntrinsicFn::MutableBitSetDifference,
        ),
        (
            Name::new(interner, "mutable_bitset_xor"),
            IntrinsicFn::MutableBitSetXor,
        ),
        // Deque
        (Name::new(interner, "deque_new"), IntrinsicFn::DequeNew),
        (
            Name::new(interner, "deque_push_front"),
            IntrinsicFn::DequePushFront,
        ),
        (
            Name::new(interner, "deque_push_back"),
            IntrinsicFn::DequePushBack,
        ),
        (
            Name::new(interner, "deque_pop_front"),
            IntrinsicFn::DequePopFront,
        ),
        (
            Name::new(interner, "deque_pop_back"),
            IntrinsicFn::DequePopBack,
        ),
        (Name::new(interner, "deque_len"), IntrinsicFn::DequeLen),
        (
            Name::new(interner, "deque_is_empty"),
            IntrinsicFn::DequeIsEmpty,
        ),
        // Seq
        (Name::new(interner, "seq_range"), IntrinsicFn::SeqRange),
        (Name::new(interner, "seq_map"), IntrinsicFn::SeqMap),
        (Name::new(interner, "seq_filter"), IntrinsicFn::SeqFilter),
        (Name::new(interner, "seq_fold"), IntrinsicFn::SeqFold),
        (Name::new(interner, "seq_scan"), IntrinsicFn::SeqScan),
        (Name::new(interner, "seq_unfold"), IntrinsicFn::SeqUnfold),
        (
            Name::new(interner, "seq_enumerate"),
            IntrinsicFn::SeqEnumerate,
        ),
        (Name::new(interner, "seq_zip"), IntrinsicFn::SeqZip),
        (Name::new(interner, "seq_chunks"), IntrinsicFn::SeqChunks),
        (Name::new(interner, "seq_windows"), IntrinsicFn::SeqWindows),
        (Name::new(interner, "seq_count"), IntrinsicFn::SeqCount),
        (Name::new(interner, "seq_count_by"), IntrinsicFn::SeqCountBy),
        (Name::new(interner, "seq_contains"), IntrinsicFn::SeqContains),
        (
            Name::new(interner, "seq_frequencies"),
            IntrinsicFn::SeqFrequencies,
        ),
        (Name::new(interner, "seq_any"), IntrinsicFn::SeqAny),
        (Name::new(interner, "seq_all"), IntrinsicFn::SeqAll),
        (Name::new(interner, "seq_find"), IntrinsicFn::SeqFind),
        (Name::new(interner, "seq_to_list"), IntrinsicFn::SeqToList),
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
            Name::new(interner, "option_unwrap_or"),
            IntrinsicFn::OptionUnwrapOr,
        ),
        (
            Name::new(interner, "option_map_or"),
            IntrinsicFn::OptionMapOr,
        ),
        (Name::new(interner, "option_map"), IntrinsicFn::OptionMap),
        (
            Name::new(interner, "option_and_then"),
            IntrinsicFn::OptionAndThen,
        ),
        (
            Name::new(interner, "result_unwrap_or"),
            IntrinsicFn::ResultUnwrapOr,
        ),
        (Name::new(interner, "result_map"), IntrinsicFn::ResultMap),
        (
            Name::new(interner, "result_and_then"),
            IntrinsicFn::ResultAndThen,
        ),
        (
            Name::new(interner, "result_map_err"),
            IntrinsicFn::ResultMapErr,
        ),
        (
            Name::new(interner, "result_map_or"),
            IntrinsicFn::ResultMapOr,
        ),
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
        (Name::new(interner, "string_md5"), IntrinsicFn::StringMd5),
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
        (Name::new(interner, "char_code"), IntrinsicFn::CharCode),
        (
            Name::new(interner, "char_is_decimal_digit"),
            IntrinsicFn::CharIsDecimalDigit,
        ),
        (
            Name::new(interner, "char_to_decimal_digit"),
            IntrinsicFn::CharToDecimalDigit,
        ),
        (
            Name::new(interner, "char_to_digit"),
            IntrinsicFn::CharToDigit,
        ),
        // Int/Float
        (Name::new(interner, "abs"), IntrinsicFn::Abs),
        (Name::new(interner, "int_pow"), IntrinsicFn::IntPow),
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
    use crate::value::SeqPlan;

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

    #[test]
    fn seq_range_returns_lazy_source_plan() {
        let range = IntrinsicFn::SeqRange
            .call(smallvec![Value::Int(0), Value::Int(8_192)])
            .expect("seq_range should succeed");
        let Value::Seq(plan) = range else {
            panic!("expected seq value");
        };
        match plan.as_ref() {
            SeqPlan::Source(SeqSource::Range { start, end }) => {
                assert_eq!((*start, *end), (0, 8_192));
            }
            other => panic!("expected range source, got {other:?}"),
        }
    }
}
