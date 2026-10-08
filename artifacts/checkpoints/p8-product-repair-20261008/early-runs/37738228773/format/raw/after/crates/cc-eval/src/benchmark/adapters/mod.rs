//! Adapters receive only public request fields and admitted source metadata.
pub mod mcp_stdio;
#[cfg(feature = "eval-http")]
pub mod oce_http;
pub mod rg;
use super::{manifest::FileRecord, readiness::Readiness, schema::SearchInput, Result};
use serde_json::Value;
#[allow(async_fn_in_trait)]
pub trait Backend {
    fn name(&self) -> &'static str;
    fn pid(&self) -> Option<u32> {
        None
    }
    async fn prepare(&mut self, files: &[FileRecord]) -> Result<Value>;
    async fn readiness(&mut self, files: &[FileRecord]) -> Result<Readiness>;
    /// Optional raw readiness observations retained even when measurement fails.
    fn readiness_observation(&self) -> Option<&Value> {
        None
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value>;
    async fn close(&mut self) -> Result<()>;
}
