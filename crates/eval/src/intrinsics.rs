//! Built-in functions available to Kyokara programs.

use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;

use crate::error::RuntimeError;
use crate::value::Value;

/// Identifies an intrinsic function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicFn {
    Print,
    Println,
    IntToString,
    StringConcat,
}

impl IntrinsicFn {
    /// Execute the intrinsic with the given arguments.
    pub fn call(self, args: Vec<Value>) -> Result<Value, RuntimeError> {
        match self {
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
        }
    }
}

/// All intrinsic name-function pairs.
pub fn all_intrinsics(interner: &mut Interner) -> Vec<(Name, IntrinsicFn)> {
    vec![
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
    ]
}
