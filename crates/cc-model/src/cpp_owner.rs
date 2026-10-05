//! Bounded C++ qualified-declaration provenance. A proven declaration is not a
//! proven call target; every B1 state remains ineligible for generic binding.
use crate::{
    id::StableId,
    source::{ByteSpan, SourceIdentity, SourceSnapshot},
    symbol::{SymbolKind, SymbolRecord},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CppQualifiedOwnerState {
    #[default]
    NonB1,
    ProvenNamespace,
    ProvenType,
    Unproven,
    Ambiguous,
}
impl CppQualifiedOwnerState {
    pub fn is_non_b1(&self) -> bool {
        *self == Self::NonB1
    }
    pub fn is_b1(self) -> bool {
        self != Self::NonB1
    }
    pub fn is_proven(self) -> bool {
        matches!(self, Self::ProvenNamespace | Self::ProvenType)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonB1 => "non_b1",
            Self::ProvenNamespace => "proven_namespace",
            Self::ProvenType => "proven_type",
            Self::Unproven => "unproven",
            Self::Ambiguous => "ambiguous",
        }
    }
    /// Unknown persistent values are errors, never defaults or positive proof.
    pub fn from_str_strict(value: &str) -> Option<Self> {
        Some(match value {
            "non_b1" => Self::NonB1,
            "proven_namespace" => Self::ProvenNamespace,
            "proven_type" => Self::ProvenType,
            "unproven" => Self::Unproven,
            "ambiguous" => Self::Ambiguous,
            _ => return None,
        })
    }
}

/// Transient declaration evidence, produced only against the current parser
/// snapshot. Persistent symbol eligibility is not a substitute for this proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CppQualifiedOwnerProof {
    pub source: SourceIdentity,
    pub definition: ByteSpan,
    pub owner_declaration: ByteSpan,
    pub owner_path: Vec<String>,
    pub state: CppQualifiedOwnerState,
    pub symbol_id: String,
    pub qname: String,
    pub symbol_uid: String,
}
impl CppQualifiedOwnerProof {
    pub fn matches(
        &self,
        source: &SourceSnapshot<'_>,
        span: ByteSpan,
        symbol: &SymbolRecord,
    ) -> bool {
        let expected_kind = match self.state {
            CppQualifiedOwnerState::ProvenNamespace => SymbolKind::Function,
            CppQualifiedOwnerState::ProvenType => SymbolKind::Method,
            _ => return false,
        };
        self.source == *source.identity()
            && self.definition == span
            && source.point(span.start).ok()
                == Some((symbol.start_line as usize, symbol.start_col as usize))
            && source.point(span.end).ok()
                == Some((symbol.end_line as usize, symbol.end_col as usize))
            && !self.owner_declaration.is_empty()
            && self.owner_declaration.end <= span.start
            && source.slice(self.owner_declaration).is_ok()
            && !self.owner_path.is_empty()
            && self.owner_path.len() <= 64
            && self
                .owner_path
                .iter()
                .all(|part| !part.is_empty() && !part.contains("::"))
            && self.state == symbol.cpp_qualified_owner
            && symbol.kind == expected_kind
            && self.symbol_id == symbol.symbol_id
            && self.qname == format!("{}::{}", self.owner_path.join("::"), symbol.name)
            && symbol.qname.as_deref() == Some(self.qname.as_str())
            && symbol.symbol_uid.as_deref() == Some(self.symbol_uid.as_str())
            && self.symbol_uid
                == StableId::symbol_uid(
                    &symbol.file_path,
                    &self.qname,
                    expected_kind.as_str(),
                    symbol.signature.as_deref(),
                )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_state_roundtrips_and_unknown_states_are_rejected() {
        for state in [
            CppQualifiedOwnerState::NonB1,
            CppQualifiedOwnerState::ProvenNamespace,
            CppQualifiedOwnerState::ProvenType,
            CppQualifiedOwnerState::Unproven,
            CppQualifiedOwnerState::Ambiguous,
        ] {
            assert_eq!(
                CppQualifiedOwnerState::from_str_strict(state.as_str()),
                Some(state)
            );
            assert_eq!(
                serde_json::from_str::<CppQualifiedOwnerState>(
                    &serde_json::to_string(&state).unwrap()
                )
                .unwrap(),
                state
            );
        }
        assert!(CppQualifiedOwnerState::from_str_strict("future_positive").is_none());
        assert!(serde_json::from_str::<CppQualifiedOwnerState>("\"future_positive\"").is_err());
    }
}
