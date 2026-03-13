// Stage 1: Core data structures for Pipelined HotStuff simulation.
// No consensus logic here — just the types that flow through the system.

use sha2::{Digest, Sha256};

// --- NodeId ---

// Newtype pattern: wraps u64 so NodeId and u64 are distinct types at compile time.
// Prevents accidentally passing a raw u64 where a NodeId is expected.
// The inner field is public so callers can do `node_id.0` to get the raw value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

// --- Block ---

// `Clone` lets us duplicate a Block when we need to send it to multiple recipients
// (channels move by default; Clone gives us an explicit copy).
// `Debug` lets us print it with `{:?}` for logging — essential for simulation output.
#[derive(Clone, Debug)]
pub struct Block {
    pub height: u64,

    // Fixed-size byte arrays ([u8; 32]) live entirely on the stack — no heap allocation.
    // SHA-256 output is always 32 bytes, so this is the natural fit.
    pub parent_hash: [u8; 32],

    pub proposer: NodeId,

    // String is heap-allocated (owned). We use String (not &str) because Block
    // owns its payload — we don't want lifetime annotations at this stage.
    pub payload: String,
}

impl Block {
    /// Returns the SHA-256 hash of this block's fields.
    ///
    /// We hash: height || parent_hash || proposer_id || payload.
    /// `&self` borrows the block immutably — no ownership transfer needed just to hash.
    pub fn block_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Sha256::update() accepts &[u8]. We convert each field to bytes:
        // u64 → little-endian 8 bytes (consistent byte order matters for determinism).
        hasher.update(self.height.to_le_bytes());
        hasher.update(self.parent_hash);
        hasher.update(self.proposer.0.to_le_bytes()); // .0 extracts the inner u64
        hasher.update(self.payload.as_bytes());

        // finalize() consumes the hasher and returns a GenericArray<u8, U32>.
        // into() converts it to [u8; 32] via the From impl in the sha2 crate.
        hasher.finalize().into()
    }
}

// --- Vote ---

#[derive(Clone, Debug)]
pub struct Vote {
    // The hash of the block this vote is for.
    pub block_hash: [u8; 32],
    pub voter: NodeId,
    pub height: u64,
}

// --- QuorumCertificate ---

// A QC is formed once 2f+1 matching votes are collected.
// `votes: Vec<Vote>` — Vec is heap-allocated, sized at runtime.
// We own the votes outright (no references) to keep lifetimes simple.
#[derive(Clone, Debug)]
pub struct QuorumCertificate {
    pub block_hash: [u8; 32],
    pub height: u64,
    pub votes: Vec<Vote>,
}

// --- TimeoutMsg ---

// Sent when a node's round timer fires before seeing a QC.
// `last_voted_block_hash` is Option<[u8; 32]>: None means the node hasn't voted yet
// at this height (e.g. it just started or is catching up).
#[derive(Clone, Debug)]
pub struct TimeoutMsg {
    pub height: u64,
    pub sender: NodeId,
    pub last_voted_block_hash: Option<[u8; 32]>,
}

// --- Message ---

// An enum that tags each message variant with its type.
// Enums in Rust are sum types — a Message is *exactly one* of these variants.
// This lets the message bus dispatch without unsafe casting.
//
// We use tuple-style variants (e.g. `Proposal(Block)`) — the payload is owned
// and moved into the enum when constructed.
//
// Note: we can't name a variant `Vote` if `Vote` is also a type in scope,
// so we use `VoteMsg` for the variant to avoid the name clash.
#[derive(Clone, Debug)]
pub enum Message {
    Proposal(Block),
    VoteMsg(Vote),
    Timeout(TimeoutMsg),
    // Broadcast by the leader once a QC is formed.
    // Every node updates its locked_qc on receipt.
    // The next leader also kicks off the next proposal.
    NewQC(QuorumCertificate),
}

// --- Envelope ---

// Wraps a Message with routing metadata (who sent it, who should receive it).
// The MessageBus operates on Envelopes, not raw Messages, so transformers
// (drop, delay, reorder) can inspect routing without touching the payload.
#[derive(Clone, Debug)]
pub struct Envelope {
    pub from: NodeId,
    pub to: NodeId,
    pub msg: Message,
}

// --- Command ---

// Output type returned by NodeState::handle().
// Pure value — no channels, no I/O. The simulation runner (Stage 3) acts on these.
// Keeping effects out of NodeState makes it easy to unit test: assert on Vec<Command>.
#[derive(Clone, Debug)]
pub enum Command {
    // Send a message to one specific node.
    SendTo { to: NodeId, msg: Message },
    // Send a message to all nodes in the cluster.
    Broadcast(Message),
    // A block has reached 2f+1 votes and is now committed to the chain.
    Commit(Block),
}
