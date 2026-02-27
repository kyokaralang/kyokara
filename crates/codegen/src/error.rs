//! Codegen error types.

/// Errors that can occur during WASM code generation.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("unsupported type for WASM codegen: {0}")]
    UnsupportedType(String),

    #[error("unsupported control flow pattern")]
    UnsupportedControlFlow,

    #[error("missing function: {0}")]
    MissingFunction(String),

    #[error("missing entry block")]
    MissingEntryBlock,

    #[error("block missing terminator: {0}")]
    MissingTerminator(String),

    #[error("unsupported instruction: {0}")]
    UnsupportedInstruction(String),

    #[error("ADT type definition not found")]
    MissingAdtDef,

    #[error("unknown variant: {0}")]
    UnknownVariant(String),
}
