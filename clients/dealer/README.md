# juodzekas-dealer

Automated dealer daemon for the juodzekas on-chain blackjack contract. Creates games, responds to player actions, and submits card reveals.

## What It Does

Runs a loop that:

1. Creates a new game on-chain (shuffle + ZK proof + bankroll deposit)
2. Waits for a player to join
3. Automatically submits card reveals when the contract requests them
4. Handles dealer turn logic (reveal hole card, hit until >= 17)
5. Claims timeout if player goes inactive
6. Optionally loops to create the next game (`AUTO_CREATE_GAME=true`)

Saves per-game ElGamal keypairs to `data/game_{id}_keys.bin` so it can resume reveals after restart.

## Prerequisites

- Funded Xion testnet-2 dealer wallet
- Deployed juodzekas contract with registered ZK verification keys
- Circuit artifacts at `circuits/circuit-artifacts/`:
  - `wasm/encrypt.wasm`, `wasm/decrypt.wasm`
  - `zkey/encrypt.zkey` (~173 MB), `zkey/decrypt.zkey`
- C++ toolchain for `rapidsnark` compilation (cmake, GMP library)

## Build

```bash
cargo build -p juodzekas-dealer --release
```

## Run

```bash
# Configure via .env or environment variables
export DEALER_MNEMONIC="word1 word2 ... word24"
export CONTRACT_ADDR="xion1..."
export RPC_URL="https://rpc.xion-testnet-2.burnt.com:443"
export CHAIN_ID="xion-testnet-2"
export BANKROLL_AMOUNT=1000000    # uxion to deposit per game (must be >= 10 * max_bet)
export AUTO_CREATE_GAME=true      # loop creating games

cargo run -p juodzekas-dealer --release
```

## Dependencies

- `mob` (burnt-labs, wallet + chain client)
- `tendermint-rpc` 0.37 (direct queries)
- `zk-shuffle` (workspace, crypto + proofs)
- `blackjack` (workspace, game rules)
- `ark-*` 0.5 (ZK primitives)
- `bip39` 2.0 (wallet from mnemonic)

## Source Layout

```
src/
  main.rs    Single-file daemon: wallet setup, game creation loop, reveal polling
```
