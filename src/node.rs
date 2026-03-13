// Stage 2: NodeState — pure state machine.
// Takes a Message, returns Vec<Command>. No async, no channels, no I/O.
// This makes it trivially unit-testable: given input X, assert output Y.

use std::collections::{HashMap, HashSet};

use crate::types::{Block, Command, Message, NodeId, QuorumCertificate, Vote};

pub struct NodeState {
    pub id: NodeId,

    // Total nodes in the cluster. Used to compute quorum threshold.
    n_nodes: usize,

    // Current round. Advances when a QC is formed.
    pub height: u64,

    // Safety invariant: never vote at the same height twice.
    // HashSet<u64> gives O(1) insert and lookup.
    voted_heights: HashSet<u64>,

    // Votes received per block hash. Leader uses this to detect quorum.
    // Key: block_hash ([u8;32] implements Hash + Eq, so it works as a map key).
    // Value: all votes collected so far for that block.
    pending_votes: HashMap<[u8; 32], Vec<Vote>>,

    // Leader stores proposed blocks here so it can emit Commit(block)
    // once enough votes arrive. Keyed by block_hash.
    pending_blocks: HashMap<[u8; 32], Block>,

    // Highest QC seen. Replicas check incoming proposals against this
    // to ensure the chain never forks (safety rule).
    pub locked_qc: Option<QuorumCertificate>,
}

impl NodeState {
    pub fn new(id: NodeId, n_nodes: usize) -> Self {
        NodeState {
            id,
            n_nodes,
            height: 0,
            voted_heights: HashSet::new(),
            pending_votes: HashMap::new(),
            pending_blocks: HashMap::new(),
            locked_qc: None,
        }
    }

    // Stage 3: fixed leader (node 1) for all heights.
    // This keeps the pipeline simple while we build the bus and runner.
    // Round-robin rotation is introduced in Stage 4, alongside QC propagation.
    // The `_height` prefix suppresses the unused-variable warning.
    pub fn leader_for_height(&self, _height: u64) -> NodeId {
        NodeId(1)
    }

    // Minimum votes needed to form a QC.
    // f = max tolerable byzantine faults = floor((n-1)/3)
    // quorum = 2f+1
    // Example: n=4 → f=1 → quorum=3
    fn quorum(&self) -> usize {
        let f = (self.n_nodes - 1) / 3;
        2 * f + 1
    }

    // Primary entry point. Dispatches to the appropriate handler.
    // `&mut self` because handling a message may update our state
    // (e.g. recording that we voted at a given height).
    pub fn handle(&mut self, msg: &Message) -> Vec<Command> {
        match msg {
            Message::Proposal(block) => self.handle_proposal(block),
            Message::VoteMsg(vote) => self.handle_vote(vote),
            Message::Timeout(t) => self.handle_timeout(t),
        }
    }

    // Called by the simulation runner to kick off a round.
    // Only the current leader should call this.
    pub fn propose(&mut self) -> Vec<Command> {
        // Build parent_hash from our locked QC, or use the zero hash for genesis.
        // `as_ref()` borrows the Option content without moving it out of self.
        let parent_hash = self
            .locked_qc
            .as_ref()
            .map(|qc| qc.block_hash)
            .unwrap_or([0u8; 32]);

        let block = Block {
            height: self.height,
            parent_hash,
            proposer: self.id.clone(),
            payload: format!("block-{}", self.height),
        };

        // Store so we can Commit when votes arrive later.
        // block_hash() is computed twice (once here, once when votes reference it),
        // but this is fine for a simulation — no performance concern.
        self.pending_blocks
            .insert(block.block_hash(), block.clone());

        vec![Command::Broadcast(Message::Proposal(block))]
    }

    fn handle_proposal(&mut self, block: &Block) -> Vec<Command> {
        // Ignore stale proposals (we've already moved past this height).
        if block.height < self.height {
            return vec![];
        }

        // Safety: never vote at the same height twice.
        if self.voted_heights.contains(&block.height) {
            return vec![];
        }

        // Safety rule (from HotStuff): a replica only votes if the proposal
        // extends its locked QC. This prevents the chain from forking.
        // Exception: height 0 (genesis) has no parent to check.
        if let Some(ref qc) = self.locked_qc {
            if block.parent_hash != qc.block_hash {
                // Proposal doesn't build on our lock — reject.
                return vec![];
            }
        }

        // All checks passed. Record the vote and send it to the leader.
        self.voted_heights.insert(block.height);

        let vote = Vote {
            block_hash: block.block_hash(),
            voter: self.id.clone(), // clone: NodeId is a newtype; we keep self.id too
            height: block.height,
        };

        let leader = self.leader_for_height(block.height);

        vec![Command::SendTo {
            to: leader,
            msg: Message::VoteMsg(vote),
        }]
    }

    fn handle_vote(&mut self, vote: &Vote) -> Vec<Command> {
        // Only the leader for this height collects votes.
        let leader = self.leader_for_height(vote.height);
        if self.id != leader {
            return vec![];
        }

        // Compute quorum BEFORE taking a mutable borrow of pending_votes.
        // If we called self.quorum() after `entry()`, the compiler would see
        // two borrows of `self` active at once (&mut via `votes`, & via quorum()),
        // which Rust forbids even though they touch different fields.
        let quorum = self.quorum();

        // Accumulate the vote and snapshot the vec if quorum is reached.
        // Scoped block so the &mut borrow of pending_votes ends before
        // we call remove() on the same map below.
        let votes_snapshot = {
            let votes = self
                .pending_votes
                .entry(vote.block_hash)
                .or_insert_with(Vec::new);

            votes.push(vote.clone());

            if votes.len() < quorum {
                return vec![]; // borrow ends here; map borrow released
            }

            votes.clone() // snapshot before we drop the borrow
        }; // <-- &mut borrow of pending_votes fully released here

        // Remove the entry so a late-arriving 4th vote doesn't trigger a second QC.
        self.pending_votes.remove(&vote.block_hash);

        // Quorum reached — form the QC.
        let qc = QuorumCertificate {
            block_hash: vote.block_hash,
            height: vote.height,
            votes: votes_snapshot,
        };

        // Update locked QC and advance height.
        self.locked_qc = Some(qc.clone());
        self.height = vote.height + 1;

        let mut commands = vec![];

        // Emit Commit if we have the block (we should — we proposed it).
        if let Some(block) = self.pending_blocks.get(&vote.block_hash) {
            commands.push(Command::Commit(block.clone()));
        }

        // If we're also the leader of the next height, propose immediately.
        // Otherwise the runner will call propose() on whoever is next leader.
        let next_leader = self.leader_for_height(self.height);
        if self.id == next_leader {
            commands.extend(self.propose());
        }

        commands
    }

    fn handle_timeout(&mut self, _t: &crate::types::TimeoutMsg) -> Vec<Command> {
        // Stub — timeout logic (view change) comes in a later stage.
        // For now just advance height so the simulation doesn't stall.
        self.height += 1;
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------
// `#[cfg(test)]` means this module is only compiled when running `cargo test`.
// Tests live next to the code they test — idiomatic Rust.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    // Helper: build a 4-node cluster (f=1, quorum=3).
    // Returns NodeState for the given id.
    fn make_node(id: u64) -> NodeState {
        NodeState::new(NodeId(id), 4)
    }

    #[test]
    fn replica_votes_on_valid_proposal() {
        let mut node = make_node(2); // node 2, not the leader at height 0

        // Leader at height 0 is node 1 (0 % 4 + 1 = 1).
        let block = Block {
            height: 0,
            parent_hash: [0u8; 32],
            proposer: NodeId(1),
            payload: "block-0".to_string(),
        };

        let cmds = node.handle(&Message::Proposal(block.clone()));

        // Should produce exactly one SendTo(leader, VoteMsg).
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::SendTo {
                to,
                msg: Message::VoteMsg(v),
            } => {
                assert_eq!(*to, NodeId(1)); // sent to leader
                assert_eq!(v.voter, NodeId(2)); // voter is us
                assert_eq!(v.height, 0);
                assert_eq!(v.block_hash, block.block_hash());
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn no_double_vote_at_same_height() {
        let mut node = make_node(2);

        let block = Block {
            height: 0,
            parent_hash: [0u8; 32],
            proposer: NodeId(1),
            payload: "block-0".to_string(),
        };

        // First proposal — should vote.
        let cmds1 = node.handle(&Message::Proposal(block.clone()));
        assert_eq!(cmds1.len(), 1);

        // Same height again (e.g. equivocating leader) — must not vote.
        let cmds2 = node.handle(&Message::Proposal(block.clone()));
        assert!(cmds2.is_empty(), "must not double-vote at same height");
    }

    #[test]
    fn leader_forms_qc_and_commits_on_quorum() {
        let mut leader = make_node(1); // leader at height 0

        // Leader proposes genesis.
        let propose_cmds = leader.propose();
        assert_eq!(propose_cmds.len(), 1);

        // Extract the proposed block from the Broadcast command.
        let block = match &propose_cmds[0] {
            Command::Broadcast(Message::Proposal(b)) => b.clone(),
            other => panic!("expected Broadcast(Proposal), got {:?}", other),
        };

        let hash = block.block_hash();

        // Send 2 votes — below quorum (need 3 of 4).
        for voter_id in [2u64, 3u64] {
            let cmds = leader.handle(&Message::VoteMsg(Vote {
                block_hash: hash,
                voter: NodeId(voter_id),
                height: 0,
            }));
            assert!(
                cmds.is_empty(),
                "should not commit yet at {} votes",
                voter_id - 1
            );
        }

        // 3rd vote reaches quorum.
        let cmds = leader.handle(&Message::VoteMsg(Vote {
            block_hash: hash,
            voter: NodeId(4),
            height: 0,
        }));

        // Expect [Commit(block), Broadcast(next_proposal)]
        // (leader at height 1 is node 2, so no second Broadcast here)
        assert!(
            cmds.iter().any(|c| matches!(c, Command::Commit(_))),
            "expected Commit in {:?}",
            cmds
        );

        assert_eq!(leader.height, 1, "height should advance after QC");
        assert!(leader.locked_qc.is_some(), "locked_qc should be set");
    }

    #[test]
    fn replica_rejects_proposal_not_extending_locked_qc() {
        let mut node = make_node(2);

        // Simulate node having a locked QC at height 0 for some block hash.
        let fake_hash = [0xaau8; 32];
        node.locked_qc = Some(QuorumCertificate {
            block_hash: fake_hash,
            height: 0,
            votes: vec![],
        });
        node.height = 1;

        // Proposal with a different parent_hash — should be rejected.
        let bad_block = Block {
            height: 1,
            parent_hash: [0xbbu8; 32], // does not match locked_qc.block_hash
            proposer: NodeId(2),       // leader at height 1
            payload: "fork-attempt".to_string(),
        };

        let cmds = node.handle(&Message::Proposal(bad_block));
        assert!(
            cmds.is_empty(),
            "should reject proposal that doesn't extend locked QC"
        );
    }
}
