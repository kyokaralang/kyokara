use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::replay::{EffectKind, RequiredByKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCheck {
    pub capability: String,
    pub required_by_kind: RequiredByKind,
    pub required_by_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectRequest {
    pub capability: String,
    pub operation: String,
    pub effect_kind: EffectKind,
    pub required_by_name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectResponse {
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum RuntimeEffectError {
    #[error("capability denied: {capability} (required by `{required_by_name}`)")]
    CapabilityDenied {
        capability: String,
        required_by_name: String,
    },
    #[error("{operation}: {message}")]
    OperationFailed { operation: String, message: String },
    #[error("replay log error: {0}")]
    ReplayLog(String),
}

pub trait RuntimeService {
    fn authorize(&mut self, check: CapabilityCheck) -> Result<(), RuntimeEffectError>;
    fn call(&mut self, request: EffectRequest) -> Result<EffectResponse, RuntimeEffectError>;

    fn finalize(&mut self) -> Result<(), RuntimeEffectError> {
        Ok(())
    }
}
