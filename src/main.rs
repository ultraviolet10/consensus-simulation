mod bus;
mod node;
mod runner;
mod transformer;
mod types;

use runner::SimulationRunner;
use transformer::PassthruTransformer;

// `#[tokio::main]` is a proc-macro that rewrites main() into:
//   fn main() { tokio::runtime::Runtime::new().unwrap().block_on(async { ... }) }
// It sets up the async runtime so we can use `.await` inside main.
#[tokio::main]
async fn main() {
    println!("=== Stage 3: MessageBus + PassthruTransformer ===\n");

    // 4 nodes, happy path (no drops/delays), run until 3 blocks committed.
    SimulationRunner::new(4, PassthruTransformer).run(3).await;
}
