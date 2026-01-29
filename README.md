# Juodžekas - Trustless Blackjack on Xion

Juodžekas [ˈjuːoˑd͡ʒɛkɐs] is a decentralized, trustless Blackjack game. It uses `zkShuffle` (Mental Poker) and optional smart contracts to ensure fair play and verifiable card dealing.

## Project Structure

This is a Rust workspace consisting of multiple components:

- `contracts/juodzekas`: The CosmWasm smart contract (WIP)
- `clients/tui`: A terminal-based user interface for playing the game
- `packages/zk-shuffle`: ZK-based card shuffling using Mental Poker
- `packages/blackjack`: Shared blackjack game logic and rules

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)

## Getting Started

### Clone with Submodules

This project uses git submodules for external dependencies (circomlib). Clone with:

```bash
git clone --recursive <repository-url>
```

Or if already cloned:

```bash
git submodule update --init --recursive
```

See [SUBMODULES.md](SUBMODULES.md) for more details.

### Smart Contract

To compile and test the smart contract:

```bash
# Compile the contract
cargo build -p juodzekas

# Run contract tests
cargo test -p juodzekas
```

### TUI Client

The TUI client is a terminal application built with `ratatui`.

To run the TUI client:

```bash
cargo run -p juodzekas-tui
```

#### Quick Start

1. Choose game mode (Fast or Trustless)
2. Select number of spots (1-8 hands to play simultaneously)
3. Use arrow keys or letter keys to play

For detailed controls and features, see [clients/tui/README.md](clients/tui/README.md).

## Development

- **Formatting**: `cargo fmt`
- **Linting**: `cargo clippy`

## ZK Circuits

The project includes the ZK circuits required for trustless card games, located in the `circuits/` directory. These circuits are based on the [zkShuffle](https://github.com/burnt-labs/zkShuffle) protocol and implement Mental Poker techniques using BabyJubJub and ElGamal.

See [circuits/README.md](circuits/README.md) for more details.
