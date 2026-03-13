// Stage 3: Transformer trait and implementations.
//
// A Transformer sits in the MessageBus pipeline. It receives an Envelope
// and either returns Some(Envelope) (pass it on, possibly modified)
// or None (drop it entirely).
//
// `Send + Sync` bounds are required because Transformers are stored inside
// MessageBus, which is wrapped in Arc and shared across tokio tasks (threads).
// `Send` = safe to move between threads. `Sync` = safe to share by reference.

use crate::types::Envelope;

pub trait Transformer: Send + Sync {
    fn transform(&self, envelope: Envelope) -> Option<Envelope>;
}

// --- PassthruTransformer ---

// No-op: every message passes through unchanged.
// Used for the happy-path simulation (all nodes online, no delays).
pub struct PassthruTransformer;

impl Transformer for PassthruTransformer {
    fn transform(&self, envelope: Envelope) -> Option<Envelope> {
        Some(envelope)
    }
}
