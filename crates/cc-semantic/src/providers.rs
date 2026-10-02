//! Worker-side embedding provider implementations (P6-009).
//!
//! The port lives in [`crate::ports`] (owner-owned freeze surface); this
//! module only hosts implementations. As of P6-009 exactly one exists:
//! [`fake`], the deterministic in-process reference provider. It never
//! touches the network — the no-network closure (P6-020 dependency check)
//! and the V15 fault matrix are both exercised through it. Real providers
//! are a later (P7) line and must not be added here.
//!
//! P7-001 adds [`openai_compatible`]: an adapter for OpenAI-compatible
//! `/embeddings` servers. It is **not** a real-network implementation — it
//! speaks through the injectable [`openai_compatible::EmbeddingHttpTransport`]
//! seam and ships fail-closed disabled (no transport configured); the live
//! leg is conditional blocked this round (user decision D1/D2).

pub mod fake;
pub mod openai_compatible;
