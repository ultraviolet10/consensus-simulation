# MonadBFT Simulation — Learning Project

## Communication Protocol

**Be extremely concise.** Minimize tokens while maximizing information density. Sacrifice complete sentences, articles (a/an/the), and grammatical formality for brevity and clarity. Use fragments, bullet points, technical shorthand. Examples:

- ❌ "I will now proceed to build the project using forge build"
- ✅ "Building with `forge build`"
- ❌ "The test has failed because there is a type mismatch error"
- ✅ "Test failed: type mismatch"
- ❌ "I have successfully completed the implementation of the new feature"
- ✅ "Feature implemented"

Apply this throughout responses—explanations, status updates, error descriptions. Every word should earn its token cost.

- Keep `CLAUDE.md` in active context for full session; re-open before substantial edits if context may have drifted.
- Surface blockers/risks first; include file paths + line numbers when citing issues.
- If unsure, ask one precise question rather than many.
- Pause to confirm intent before assumptions; do not guess.
- For reviews, clarify scope first.

## Context
Learning Rust by building a 4-node consensus simulation.
Implement Pipelined HotStuff first, then MonadBFT's tailfork fix.

## Constraints
- I'm new to Rust. Explain ownership/borrow decisions inline as comments.
- Prefer clarity over idiomatic cleverness. No macros until Stage 4.
- Use tokio for async. Use mpsc channels for inter-node messaging.
- Each stage in a separate commit. Don't skip ahead.

## Current Stage
**Stage 4 complete** — `DropTransformer` + round-robin leader rotation + `Message::NewQC` propagation.
Two scenarios: happy path (4 commits) and node 4 offline (3 commits, quorum still met).
Offline leader (node 4 at height 3) causes stall — motivates Stage 5.
Next: Stage 5 — timeout/view-change so an offline leader doesn't stall progress.

## Key Invariants
- A QC requires votes from 2f+1 nodes (f = byzantine fault tolerance)
- A node never votes for two different blocks at the same height
- On timeout: broadcast TimeoutMsg with last_voted_block_header

## Open Questions
[dump things you don't understand here, Claude Code will pick them up]

## Reference Architecture
Modeled on monad-mock-swarm pattern:
- NodeState is a pure state machine: takes Message, returns Vec<Command>
- MessageBus wraps channels with a transformer pipeline
- Transformers implement a single trait: fn transform(msg: Envelope) -> Option<Envelope>
- SimulationRunner wires N nodes, drives rounds, collects logs

## Transformer Scenarios to Implement (in order)
1. PassthruTransformer (no-op, used for happy path)
2. DropTransformer { from: NodeId } — makes a node "offline"
3. DelayTransformer { ms: u64 } — slow network  
4. TailForkTransformer — node skips building QC for previous block
