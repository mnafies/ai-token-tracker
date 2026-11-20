use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceState {
    pub offset: u64,
    pub size: u64,
    pub mtime_ms: i64,
    pub context: Value,
}

impl Default for SourceState {
    fn default() -> Self {
        SourceState { offset: 0, size: 0, mtime_ms: 0, context: Value::Null }
    }
}
