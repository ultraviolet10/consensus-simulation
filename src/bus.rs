// Stage 3: MessageBus — routes Envelopes between node tasks.
//
// Wrapped in Arc<MessageBus> so every node task can hold a reference to it
// and call send/broadcast without needing a central coordinator.
//
// Arc = Atomically Reference Counted. Like Rc but thread-safe.
// When the last Arc clone is dropped, MessageBus is deallocated.

use std::collections::HashMap;

use tokio::sync::mpsc;

use crate::transformer::Transformer;
use crate::types::{Envelope, Message, NodeId};

pub struct MessageBus {
    // One Sender per node. Sending into it queues an Envelope in that node's
    // inbox (the corresponding Receiver lives inside the node's task).
    // `mpsc` = multi-producer, single-consumer: many tasks can clone/send,
    // but only one task (the node) reads from the other end.
    senders: HashMap<NodeId, mpsc::Sender<Envelope>>,

    // The active transformer. Box<dyn Trait> = heap-allocated trait object,
    // allows runtime polymorphism (we can swap in DropTransformer later).
    transformer: Box<dyn Transformer + Send + Sync>,
}

impl MessageBus {
    pub fn new(
        senders: HashMap<NodeId, mpsc::Sender<Envelope>>,
        transformer: Box<dyn Transformer + Send + Sync>,
    ) -> Self {
        MessageBus {
            senders,
            transformer,
        }
    }

    // Run `envelope` through the transformer, then deliver it.
    // `&self` (not `&mut self`) because Arc requires shared references —
    // you can only get &T out of Arc<T>, never &mut T (without a Mutex).
    pub async fn send(&self, envelope: Envelope) {
        // transform() returns None → message is dropped (e.g. DropTransformer).
        if let Some(env) = self.transformer.transform(envelope) {
            if let Some(tx) = self.senders.get(&env.to) {
                // Sender::send() is async: waits if the channel buffer is full.
                // We ignore Err (receiver dropped = node shut down cleanly).
                let _ = tx.send(env).await;
            }
        }
    }

    // Send `msg` to every node in the cluster.
    // Collect node IDs first so we don't hold a borrow on `senders`
    // while calling `self.send()` (which also borrows `senders`).
    pub async fn broadcast(&self, from: NodeId, msg: Message) {
        let targets: Vec<NodeId> = self.senders.keys().cloned().collect();
        for to in targets {
            self.send(Envelope {
                from: from.clone(),
                to,
                msg: msg.clone(),
            })
            .await;
        }
    }
}
