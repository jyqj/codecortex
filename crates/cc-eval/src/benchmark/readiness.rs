use super::{invalid, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ready,
    Pending,
    Failed,
    Partial,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Readiness {
    pub total: usize,
    pub ready: usize,
    pub pending: usize,
    pub failed: usize,
    pub unknown: usize,
    pub state: State,
}
impl Readiness {
    pub fn new(
        total: usize,
        ready: usize,
        pending: usize,
        failed: usize,
        unknown: usize,
    ) -> Result<Self> {
        let sum = ready
            .checked_add(pending)
            .and_then(|x| x.checked_add(failed))
            .and_then(|x| x.checked_add(unknown));
        if total == 0 || sum != Some(total) {
            return Err(invalid("readiness counts must partition a nonempty input"));
        }
        let state = if ready == total {
            State::Ready
        } else if failed > 0 && ready == 0 {
            State::Failed
        } else if unknown > 0 {
            State::Unknown
        } else if ready > 0 {
            State::Partial
        } else {
            State::Pending
        };
        Ok(Self {
            total,
            ready,
            pending,
            failed,
            unknown,
            state,
        })
    }
}
