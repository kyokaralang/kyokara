//! Ty -> WASM ValType mapping.

use kyokara_hir_ty::ty::Ty;
use wasm_encoder::ValType;

use crate::error::CodegenError;

/// Map a Kyokara type to a WASM value type.
///
/// - Int -> i64
/// - Float -> f64
/// - Bool, Unit, Char, Fn -> i32
/// - String, Adt, Record -> i32 (pointer into linear memory)
/// - Error, Never -> i32 in unreachable/error paths
/// - Var -> error
pub fn ty_to_valtype(ty: &Ty) -> Result<ValType, CodegenError> {
    match ty {
        Ty::Int => Ok(ValType::I64),
        Ty::Float => Ok(ValType::F64),
        Ty::Bool | Ty::Unit | Ty::Char => Ok(ValType::I32),
        Ty::String | Ty::Adt { .. } | Ty::Record { .. } => Ok(ValType::I32), // pointer
        Ty::Fn { .. } => Ok(ValType::I32),
        // Never/Error can appear in unreachable code paths; treat as i32
        Ty::Never | Ty::Error => Ok(ValType::I32),
        Ty::Var(_) => Err(CodegenError::UnsupportedType("unresolved type var".into())),
    }
}

/// Returns true if the type is represented as i32 in WASM (Bool, Unit, pointers).
pub fn is_i32_type(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Bool
            | Ty::Unit
            | Ty::Char
            | Ty::String
            | Ty::Fn { .. }
            | Ty::Adt { .. }
            | Ty::Record { .. }
            | Ty::Never
            | Ty::Error
    )
}
