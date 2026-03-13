# consensus-simulation

A 4-node BFT consensus simulation in Rust, built for learning.
Implements Pipelined HotStuff step-by-step, then reproduces and fixes the MonadBFT tailfork attack.

**Spec:** [HotStuff: BFT Consensus with Linearity and Responsiveness](https://arxiv.org/abs/1803.05069) — Abraham, Malkhi et al. (2018)

---

## Architecture

### Message flow

```mermaid
graph LR
    subgraph Node Task
        NS[NodeState\npure state machine]
    end

    subgraph MessageBus
        T[Transformer\nPipeline]
        CH[mpsc channels\none per node]
    end

    NS -->|Vec&lt;Command&gt;| EX[execute_command]
    EX -->|Envelope| T
    T -->|Option&lt;Envelope&gt;| CH
    CH -->|Envelope| NS
```

### NodeState — pure state machine

Every node is a plain struct with no I/O. `handle(msg) -> Vec<Command>` is the only entry point.

```mermaid
stateDiagram-v2
    [*] --> Idle

    Idle --> Voted : Proposal received\n[extends locked_qc]\n[not double-vote]
    Idle --> Idle : Proposal rejected\n(stale / safety violation)

    Voted --> Idle : VoteMsg sent to leader

    Idle --> QC_Formed : VoteMsg received\n[am leader]\n[votes >= 2f+1]
    QC_Formed --> Idle : Commit emitted\nNewQC broadcast\nnext leader proposes
```

### HotStuff safety rules (where each invariant lives in the code)

```mermaid
flowchart TD
    A[Proposal arrives] --> B{height < self.height?}
    B -- yes --> DROP[drop — stale]
    B -- no --> C{already voted\nat this height?}
    C -- yes --> DROP2[drop — no double-vote\n§ HotStuff Safety Rule S1]
    C -- no --> D{locked_qc set?}
    D -- yes --> E{block.parent_hash\n== locked_qc.block_hash?}
    E -- no --> DROP3[drop — fork attempt\n§ HotStuff Safety Rule S2]
    E -- yes --> VOTE[vote and send to leader]
    D -- no --> VOTE

    VOTE --> F[leader collects votes]
    F --> G{votes >= 2f+1?}
    G -- no --> WAIT[wait]
    G -- yes --> H[form QC\nCommit block\nbroadcast NewQC\n§ HotStuff Linearity]
```

---

## Parameters

| Symbol | Value | Meaning |
|--------|-------|---------|
| `n` | 4 | total nodes |
| `f` | 1 | max byzantine faults `= floor((n-1)/3)` |
| quorum | 3 | votes needed for QC `= 2f+1` |

---

## Progress

### Stages complete

| Stage | What | Key Rust concepts |
|-------|------|-------------------|
| 1 | `types.rs` — `Block`, `Vote`, `QC`, `Message`, `Envelope`, `Command` | newtype pattern, `derive`, `[u8;32]` hashing |
| 2 | `node.rs` — `NodeState` pure state machine, 4 unit tests | ownership, `HashSet`/`HashMap`, borrow scoping |
| 3 | `bus.rs`, `runner.rs`, `transformer.rs` — async wiring | `Arc`, `tokio::mpsc`, `broadcast`, `select!` |
| 4 | `DropTransformer` + round-robin rotation + `NewQC` propagation | trait objects, `Box<dyn Trait>` |

### Stages remaining

| Stage | What |
|-------|------|
| 5 | `TimeoutMsg` handling — view-change when leader is offline |
| 6 | `DelayTransformer` — slow network, timing edge cases |
| 7 | `TailForkTransformer` — reproduce the MonadBFT attack |
| 8 | MonadBFT fix — `block.justify` QC field + proposal validation |

---

## The MonadBFT goal

### The tailfork attack (Stage 7)

In pipelined HotStuff, a leader at height `h` is supposed to embed a QC for block `h-1` in their proposal, proving the previous block is safely certified. The attack:

```mermaid
sequenceDiagram
    participant L as Honest Leader (h)
    participant M as Malicious Leader (h+1)
    participant R1 as Replica A
    participant R2 as Replica B

    L->>R1: Proposal(block-h) + QC(h-1)
    L->>R2: Proposal(block-h) + QC(h-1)
    R1->>M: Vote for block-h
    Note over R2: slow — hasn't seen block-h yet

    M->>R1: Proposal(block-h+1, parent=block-h)\n⚠ QC for block-h MISSING
    M->>R2: Proposal(block-h+1, parent=block-h)\n⚠ QC for block-h MISSING

    Note over R1: votes — already saw block-h
    Note over R2: can't verify chain — rejects or forks
```

### The fix (Stage 8)

One rule added to `handle_proposal` in `node.rs`:

```rust
// MonadBFT: proposal must include a valid QC for its parent.
// Without this, a malicious leader can fork the tail of the chain.
if !block.justify_is_valid() {
    return vec![]; // reject
}
```

This requires adding `justify: QuorumCertificate` to `Block`. A proposer must prove the previous block reached quorum before anyone will extend the chain. The tailfork becomes impossible: no valid QC → no votes → no extension.

---

## Development

### Run

```bash
cargo run
```

Output shows two scenarios: all nodes online, then node 4 offline (fault tolerance demo).

### Test

```bash
cargo test
```

### Lint (Clippy)

```bash
# Lint with warnings
cargo clippy

# Fail on any warning (use before committing)
cargo clippy -- -D warnings

# Auto-fix what it can
cargo clippy --fix
```

Lint rules configured in `Cargo.toml` under `[lints.clippy]`.
