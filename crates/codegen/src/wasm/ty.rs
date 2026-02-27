//! Ty -> WASM ValType mapping.

use kyokara_hir_ty::ty::Ty;
use wasm_encoder::ValType;

use crate::error::CodegenError;

/// Map a Kyokara type to a WASM value type.
///
/// - Int -> i64
/// - Float -> f64
/// - Bool, Unit -> i32
/// - Adt, Record -> i32 (pointer into linear memory)
/// - String, Char, Fn, Error, Never, Var -> error (deferred to follow-up)
pub fn ty_to_valtype(ty: &Ty) -> Result<ValType, CodegenError> {
    match ty {
        Ty::Int => Ok(ValType::I64),
        Ty::Float => Ok(ValType::F64),
        Ty::Bool | Ty::Unit => Ok(ValType::I32),
        Ty::Adt { .. } | Ty::Record { .. } => Ok(ValType::I32), // pointer
        // Never/Error can appear in unreachable code paths; treat as i32
        Ty::Never | Ty::Error => Ok(ValType::I32),
        Ty::String => Err(CodegenError::UnsupportedType("String".into())),
        Ty::Char => Err(CodegenError::UnsupportedType("Char".into())),
        Ty::Fn { .. } => Err(CodegenError::UnsupportedType("Fn".into())),
        Ty::Var(_) => Err(CodegenError::UnsupportedType("unresolved type var".into())),
    }
}

/// Returns true if the type is represented as i32 in WASM (Bool, Unit, pointers).
pub fn is_i32_type(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Bool | Ty::Unit | Ty::Adt { .. } | Ty::Record { .. } | Ty::Never | Ty::Error
    )
}
