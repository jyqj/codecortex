//! Versioned comparison receipts. Only a fresh complete capture can admit outputs.
use super::{
    capture_python_declarations, AdmissionPolicies, CaptureRefusal, CaptureRequest,
    CapturedDeclarations,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Wire shape version; unknown versions are never interpreted as this version.
pub const CAPTURE_RECEIPT_VERSION: u32 = 1;
/// Covers the complete capture, root provenance, pinned Python AST and model rules.
/// Changes to those derivation contracts require a new version, even if the wire
/// shape remains unchanged. This is not a source-revision or release certificate.
pub const CAPTURE_DERIVATION_VERSION: u32 = 1;
/// A fixed wire ceiling, checked before deserialization. The 4096-byte owner and
/// three fixed-size hex digests fit even when the owner needs JSON escaping.
pub const CAPTURE_RECEIPT_MAX_BYTES: usize = 16 * 1024;
const ENTIRE_PROJECT: &str = "entire_project";
const INVENTORY_DOMAIN: &str = "CodeCortex Python capture receipt v1 inventory";
const CONFIGURATION_DOMAIN: &str = "CodeCortex Python capture receipt v1 configuration";
const OUTCOMES_DOMAIN: &str = "CodeCortex Python capture receipt v1 outcomes";

/// An untrusted equality assertion suitable for serialization, never authority
/// to access a project or a substitute for `CapturedDeclarations`. Public fields
/// and deserialization deliberately cannot construct an admitted declaration.
/// Only `revalidate_python_declarations` compares it with freshly derived data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureReceipt {
    pub version: u32,
    pub derivation_version: u32,
    pub owner: String,
    pub scope: String,
    pub inventory_digest: String,
    pub configuration_digest: String,
    pub outcomes_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptMismatch {
    Configuration,
    Inventory,
    Outcomes,
}

#[derive(Debug)]
pub enum RevalidationRefusal {
    ReceiptBytes { actual: usize, limit: usize },
    MalformedReceipt,
    UnsupportedVersion,
    OwnerMismatch,
    ScopeMismatch,
    Mismatch(ReceiptMismatch),
    Capture(CaptureRefusal),
    Encoding,
}
type Result<T> = std::result::Result<T, RevalidationRefusal>;

impl CapturedDeclarations {
    /// Hashes immutable admitted contents without reading disk, cloning source
    /// bytes or serializing the whole inventory/outcomes into a temporary Vec.
    /// The receipt proves no continuing freshness; revalidation must recapture.
    pub fn receipt(&self) -> Result<CaptureReceipt> {
        let mut inventory = blake3::Hasher::new_derive_key(INVENTORY_DOMAIN);
        inventory.update(&(self.files.len() as u64).to_le_bytes());
        for (path, bytes) in &self.files {
            inventory.update(&(path.len() as u64).to_le_bytes());
            inventory.update(path.as_bytes());
            inventory.update(&(bytes.len() as u64).to_le_bytes());
            inventory.update(bytes);
        }
        Ok(CaptureReceipt {
            version: CAPTURE_RECEIPT_VERSION,
            derivation_version: CAPTURE_DERIVATION_VERSION,
            owner: self.owner.clone(),
            scope: ENTIRE_PROJECT.into(),
            inventory_digest: inventory.finalize().to_hex().to_string(),
            configuration_digest: json_digest(
                CONFIGURATION_DOMAIN,
                &(&self.config, &self.provenance),
            )?,
            outcomes_digest: json_digest(OUTCOMES_DOMAIN, &self.outcomes)?,
        })
    }
}

/// Re-admits declarations only by running the complete existing Linux capture,
/// configuration loader, AST adapter, model and final inventory/byte verification.
/// The caller supplies authorization and all three current admission budgets;
/// receipt contents never supply a root, scope permission, policies or outputs.
/// Returns the new immutable capture, never a deserialized declaration object.
pub fn revalidate_python_declarations(
    root: &Path,
    request: CaptureRequest<'_>,
    policies: AdmissionPolicies,
    receipt_json: &[u8],
) -> Result<CapturedDeclarations> {
    if receipt_json.len() > CAPTURE_RECEIPT_MAX_BYTES {
        return Err(RevalidationRefusal::ReceiptBytes {
            actual: receipt_json.len(),
            limit: CAPTURE_RECEIPT_MAX_BYTES,
        });
    }
    // Derived struct deserialization rejects duplicate/missing/unknown fields;
    // from_slice also rejects trailing non-whitespace and malformed scalar types.
    let expected: CaptureReceipt =
        serde_json::from_slice(receipt_json).map_err(|_| RevalidationRefusal::MalformedReceipt)?;
    if expected.version != CAPTURE_RECEIPT_VERSION
        || expected.derivation_version != CAPTURE_DERIVATION_VERSION
    {
        return Err(RevalidationRefusal::UnsupportedVersion);
    }
    if expected.owner != request.owner {
        return Err(RevalidationRefusal::OwnerMismatch);
    }
    if expected.scope != ENTIRE_PROJECT {
        return Err(RevalidationRefusal::ScopeMismatch);
    }
    if ![
        &expected.inventory_digest,
        &expected.configuration_digest,
        &expected.outcomes_digest,
    ]
    .into_iter()
    .all(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(RevalidationRefusal::MalformedReceipt);
    }
    let fresh = capture_python_declarations(root, request, policies)
        .map_err(RevalidationRefusal::Capture)?;
    let actual = fresh.receipt()?;
    // Configuration is checked first because its raw-byte digest is also part
    // of the complete inventory. Report the more specific changed component.
    for (matches, component) in [
        (
            actual.configuration_digest == expected.configuration_digest,
            ReceiptMismatch::Configuration,
        ),
        (
            actual.inventory_digest == expected.inventory_digest,
            ReceiptMismatch::Inventory,
        ),
        (
            actual.outcomes_digest == expected.outcomes_digest,
            ReceiptMismatch::Outcomes,
        ),
    ] {
        if !matches {
            return Err(RevalidationRefusal::Mismatch(component));
        }
    }
    Ok(fresh)
}

fn json_digest(domain: &'static str, value: &impl Serialize) -> Result<String> {
    let mut hasher = blake3::Hasher::new_derive_key(domain);
    serde_json::to_writer(&mut hasher, value).map_err(|_| RevalidationRefusal::Encoding)?;
    Ok(hasher.finalize().to_hex().to_string())
}
