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

// --- DropTransformer ---

// Silently drops all outbound messages from `drop_from`.
// Simulates a node that has gone offline or is partitioned from the network.
// Other nodes can still talk to each other — only this node's sends are blocked.
pub struct DropTransformer {
    pub drop_from: crate::types::NodeId,
}

impl Transformer for DropTransformer {
    fn transform(&self, envelope: Envelope) -> Option<Envelope> {
        if envelope.from == self.drop_from {
            // Return None = drop the message entirely.
            None
        } else {
            Some(envelope)
        }
    }
}
