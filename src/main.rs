mod bus;
mod node;
mod runner;
mod transformer;
mod types;

use runner::SimulationRunner;
use transformer::{DropTransformer, PassthruTransformer};
use types::NodeId;

#[tokio::main]
async fn main() {
    println!("=== Scenario 1: Happy path (all 4 nodes online) ===\n");
    SimulationRunner::new(4, PassthruTransformer).run(4).await;

    println!("\n=== Scenario 2: Node 4 offline (DropTransformer) ===");
    println!("f=1, quorum=3 — heights 0-2 use leaders 1,2,3 so consensus proceeds");
    println!("(height 3 would stall: leader=node4 is offline — timeout/view-change is Stage 5)\n");
    // Only 3 commits: leaders at heights 0,1,2 are nodes 1,2,3 — all online.
    SimulationRunner::new(4, DropTransformer { drop_from: NodeId(4) }).run(3).await;
}
