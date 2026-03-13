// Stage 3: SimulationRunner — wires N nodes together and drives the simulation.
//
// Each node runs in its own tokio task (lightweight green thread).
// The runner kicks off round 0, then collects commits until the target is reached.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc};

use crate::bus::MessageBus;
use crate::node::NodeState;
use crate::transformer::Transformer;
use crate::types::{Block, Command, Envelope, NodeId};

pub struct SimulationRunner {
    n_nodes: usize,
    // Box<dyn Trait + 'static> because we move it into Arc, which requires 'static.
    // The 'static bound means no borrowed data inside the transformer.
    transformer: Box<dyn Transformer + Send + Sync + 'static>,
}

impl SimulationRunner {
    // `impl Transformer + 'static` accepts any concrete type that implements
    // Transformer — we box it immediately so the struct doesn't need a type param.
    pub fn new(n_nodes: usize, transformer: impl Transformer + 'static) -> Self {
        SimulationRunner {
            n_nodes,
            transformer: Box::new(transformer),
        }
    }

    pub async fn run(self, n_commits: usize) {
        let n = self.n_nodes;

        // --- Build channels (one inbox per node) ---
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();

        for i in 1..=n as u64 {
            // Buffer size 64: up to 64 unread messages before send() blocks.
            let (tx, rx) = mpsc::channel::<Envelope>(64);
            senders.insert(NodeId(i), tx);
            receivers.insert(NodeId(i), rx);
        }

        // Arc lets every spawned task share ownership of the bus.
        let bus = Arc::new(MessageBus::new(senders, self.transformer));

        // --- Commit reporting channel ---
        // Tasks forward Commit(block) here. Runner collects until n_commits.
        let (commit_tx, mut commit_rx) = mpsc::channel::<Block>(32);

        // --- Shutdown broadcast ---
        // broadcast::channel: one sender, N subscribers (one per task).
        // When runner sends (), every task's shutdown_rx.recv() unblocks.
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        // --- Build NodeStates and kick off round 0 ---
        let mut nodes: Vec<NodeState> = (1..=n as u64)
            .map(|i| NodeState::new(NodeId(i), n))
            .collect();

        // Leader at height 0 is nodes[0] = NodeId(1).
        // Call propose() before tasks start so the proposal is queued
        // in node inboxes before they begin their recv() loops.
        let initial_cmds = nodes[0].propose();
        for cmd in initial_cmds {
            execute_command(NodeId(1), cmd, &bus, &commit_tx).await;
        }

        // --- Spawn one tokio task per node ---
        let mut handles = vec![];
        for (i, node) in nodes.into_iter().enumerate() {
            let id = NodeId(i as u64 + 1);
            let rx = receivers.remove(&id).expect("receiver missing for node");
            let bus_clone = Arc::clone(&bus);
            let commit_tx_clone = commit_tx.clone();
            // Each task gets its own Receiver end of the shutdown broadcast.
            let shutdown_rx = shutdown_tx.subscribe();

            // `move` closure: takes ownership of id, node, rx, etc.
            // Required because tokio::spawn needs 'static futures — no borrows.
            handles.push(tokio::spawn(async move {
                run_node(id, node, rx, bus_clone, commit_tx_clone, shutdown_rx).await;
            }));
        }

        // Drop our clone of commit_tx so commit_rx closes after all tasks exit.
        drop(commit_tx);

        // --- Collect commits ---
        let mut committed = 0;
        while committed < n_commits {
            match commit_rx.recv().await {
                Some(block) => {
                    committed += 1;
                    println!(
                        "[COMMIT {}/{}] height={} payload={}",
                        committed, n_commits, block.height, block.payload
                    );
                }
                None => break, // all senders dropped = all tasks exited
            }
        }

        // --- Shut down tasks ---
        // send() returns Err if no subscribers, which is fine (tasks may have exited).
        let _ = shutdown_tx.send(());
        for h in handles {
            let _ = h.await;
        }

        println!("--- simulation done: {} commits ---", committed);
    }
}

// Each node runs this loop: receive envelope → handle → execute commands.
async fn run_node(
    id: NodeId,
    mut node: NodeState,
    mut rx: mpsc::Receiver<Envelope>,
    bus: Arc<MessageBus>,
    commit_tx: mpsc::Sender<Block>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        // tokio::select! polls multiple async operations concurrently.
        // `biased` makes it check branches top-to-bottom instead of randomly —
        // ensures we drain pending messages before checking shutdown.
        tokio::select! {
            biased;

            maybe_env = rx.recv() => {
                match maybe_env {
                    // Channel closed: all senders (MessageBus) dropped.
                    None => break,
                    Some(env) => {
                        let cmds = node.handle(&env.msg);
                        for cmd in cmds {
                            execute_command(id.clone(), cmd, &bus, &commit_tx).await;
                        }
                    }
                }
            }

            // Runner sent shutdown signal.
            _ = shutdown_rx.recv() => break,
        }
    }
}

// Translate a Command into actual I/O (channel sends).
// Standalone async fn (not a method) so it can be called from both
// the runner (initial kickoff) and from node tasks.
async fn execute_command(
    from: NodeId,
    cmd: Command,
    bus: &Arc<MessageBus>,
    commit_tx: &mpsc::Sender<Block>,
) {
    match cmd {
        Command::SendTo { to, msg } => {
            bus.send(Envelope { from, to, msg }).await;
        }
        Command::Broadcast(msg) => {
            bus.broadcast(from, msg).await;
        }
        Command::Commit(block) => {
            // Ignore send error: runner may have already collected enough commits
            // and dropped its receiver.
            let _ = commit_tx.send(block).await;
        }
    }
}
