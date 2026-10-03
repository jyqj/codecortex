//! Stable, deterministic ID generation.
//!
//! All IDs are derived from semantic identity via blake3 hashing.
//! No random UUIDs — this ensures:
//! - Incremental index alignment
//! - Cross-revision symbol tracking
//! - Stable graph edges
//! - Reproducible context deltas

use blake3::Hasher;

/// Marker trait for stable ID generation functions.
pub struct StableId;

impl StableId {
    /// Generate a stable symbol UID from semantic identity.
    ///
    /// Inputs: file_path + qname + kind + normalized_signature.
    /// Resilient to line-number drift — only changes when semantic identity changes.
    pub fn symbol_uid(file_path: &str, qname: &str, kind: &str, signature: Option<&str>) -> String {
        let mut h = Hasher::new();
        h.update(b"sym:");
        h.update(file_path.as_bytes());
        h.update(b"\0");
        h.update(qname.as_bytes());
        h.update(b"\0");
        h.update(kind.as_bytes());
        if let Some(sig) = signature {
            h.update(b"\0");
            h.update(Self::normalize_signature(sig).as_bytes());
        }
        format!("uid:{}", &h.finalize().to_hex()[..24])
    }

    /// Generate a deterministic chunk ID.
    pub fn chunk_id(file_path: &str, chunk_index: u32) -> String {
        format!("chunk:{}:{}", file_path, chunk_index)
    }

    /// Generate a deterministic edge ID (call, route, test).
    pub fn edge_id(kind: &str, file_path: &str, line: u32, col: u32) -> String {
        let mut h = Hasher::new();
        h.update(kind.as_bytes());
        h.update(b":");
        h.update(file_path.as_bytes());
        h.update(b":");
        h.update(&line.to_le_bytes());
        h.update(&col.to_le_bytes());
        format!("{}:{}", kind, &h.finalize().to_hex()[..16])
    }

    /// Generate a deterministic symbol reference ID.
    pub fn ref_id(file_path: &str, symbol_name: &str, line: u32, col: u32) -> String {
        let mut h = Hasher::new();
        h.update(b"ref:");
        h.update(file_path.as_bytes());
        h.update(b"\0");
        h.update(symbol_name.as_bytes());
        h.update(b":");
        h.update(&line.to_le_bytes());
        h.update(&col.to_le_bytes());
        format!("ref:{}", &h.finalize().to_hex()[..16])
    }

    /// Generate a deterministic co-change edge ID from two file paths.
    pub fn cochange_edge_id(file_a: &str, file_b: &str) -> String {
        let mut h = Hasher::new();
        h.update(b"cochange:");
        h.update(file_a.as_bytes());
        h.update(b"\0");
        h.update(file_b.as_bytes());
        format!("cochange:{}", &h.finalize().to_hex()[..16])
    }

    /// Generate a deterministic test edge ID.
    pub fn test_edge_id(test_file: &str, code_file: &str, reason: &str) -> String {
        let mut h = Hasher::new();
        h.update(b"test:");
        h.update(test_file.as_bytes());
        h.update(b"\0");
        h.update(code_file.as_bytes());
        h.update(b"\0");
        h.update(reason.as_bytes());
        format!("test:{}", &h.finalize().to_hex()[..16])
    }

    /// Normalize a signature: collapse whitespace, trim.
    fn normalize_signature(sig: &str) -> String {
        sig.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// Short hash of whitespace-normalized text (blake3, first 12 hex chars).
/// Useful for span identity.
pub fn span_text_hash(text: &str) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let hash = blake3::hash(normalized.as_bytes());
    hash.to_hex()[..12].to_string()
}

/// Deterministic u64 hash using blake3 (same hasher family as StableId).
pub fn stable_hash(text: &str) -> u64 {
    let hash = blake3::hash(text.as_bytes());
    let bytes: [u8; 8] = hash.as_bytes()[..8].try_into().unwrap();
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_uid_is_deterministic() {
        let a = StableId::symbol_uid("src/main.rs", "main", "function", None);
        let b = StableId::symbol_uid("src/main.rs", "main", "function", None);
        assert_eq!(a, b);
        assert!(a.starts_with("uid:"));
        assert_eq!(a.len(), 4 + 24); // "uid:" + 24 hex chars
    }

    #[test]
    fn symbol_uid_differs_by_kind() {
        let a = StableId::symbol_uid("src/lib.rs", "Foo", "class", None);
        let b = StableId::symbol_uid("src/lib.rs", "Foo", "function", None);
        assert_ne!(a, b);
    }

    #[test]
    fn symbol_uid_signature_normalized() {
        let a = StableId::symbol_uid("f.ts", "foo", "function", Some("(a: number,  b: string)"));
        let b = StableId::symbol_uid("f.ts", "foo", "function", Some("(a: number, b: string)"));
        assert_eq!(a, b);
    }

    #[test]
    fn chunk_id_is_deterministic() {
        let a = StableId::chunk_id("src/main.rs", 3);
        let b = StableId::chunk_id("src/main.rs", 3);
        assert_eq!(a, b);
        assert_eq!(a, "chunk:src/main.rs:3");
    }

    #[test]
    fn edge_id_is_deterministic() {
        let a = StableId::edge_id("call", "src/main.rs", 42, 5);
        let b = StableId::edge_id("call", "src/main.rs", 42, 5);
        assert_eq!(a, b);
        assert!(a.starts_with("call:"));
    }

    #[test]
    fn ref_id_is_deterministic() {
        let a = StableId::ref_id("src/lib.rs", "process", 10, 4);
        let b = StableId::ref_id("src/lib.rs", "process", 10, 4);
        assert_eq!(a, b);
        assert!(a.starts_with("ref:"));
    }

    #[test]
    fn span_text_hash_deterministic() {
        let a = span_text_hash("hello  world");
        let b = span_text_hash("hello world");
        assert_eq!(a, b); // whitespace normalized
        assert_eq!(a.len(), 12);

        let c = span_text_hash("different text");
        assert_ne!(a, c);
    }

    #[test]
    fn stable_hash_deterministic() {
        let a = stable_hash("hello");
        let b = stable_hash("hello");
        assert_eq!(a, b);

        let c = stable_hash("world");
        assert_ne!(a, c);
    }
}
