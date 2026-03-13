# consensus-simulation

A 4-node consensus simulation built in Rust for learning purposes.
Implements Pipelined HotStuff, then MonadBFT's tailfork fix.

## Development

### Linting (Clippy)

```bash
# Lint with warnings
cargo clippy

# Fail on any warning (use before committing)
cargo clippy -- -D warnings

# Auto-fix what it can
cargo clippy --fix

# Stricter pedantic lints
cargo clippy -- -W clippy::pedantic
```

Project-level lint rules are configured in `Cargo.toml` under `[lints.clippy]`.
