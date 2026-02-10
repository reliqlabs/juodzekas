# juodzekas (Smart Contract)

CosmWasm smart contract implementing on-chain blackjack with ZK proof verification. Deployed to Xion testnet-2.

## What It Does

Manages the full lifecycle of a blackjack game between a dealer and player:

1. **CreateGame** - Dealer deposits bankroll (`10 * max_bet`), submits shuffled encrypted deck + ZK shuffle proof
2. **JoinGame** - Player places bet, submits re-shuffled deck + ZK shuffle proof
3. **SubmitReveal** - Both parties submit partial decryptions (with ZK reveal proofs) to reveal cards
4. **Player Actions** - Hit, Stand, DoubleDown, Split, Surrender
5. **Settlement** - Automatic payout when game concludes
6. **ClaimTimeout** - Claim funds if opponent goes inactive
7. **SweepSettled** - Permissionless cleanup of old settled games

All shuffle and reveal operations are verified on-chain via Xion's ZK module (Groth16 proofs over BabyJubJub).

## Contract Messages

```
ExecuteMsg::CreateGame { public_key, shuffled_deck, proof, public_inputs }
ExecuteMsg::JoinGame { game_id, bet, public_key, shuffled_deck, proof, public_inputs }
ExecuteMsg::Hit/Stand/DoubleDown/Split/Surrender { game_id }
ExecuteMsg::SubmitReveal { game_id, card_index, partial_decryption, proof, public_inputs }
ExecuteMsg::ClaimTimeout { game_id }
ExecuteMsg::SweepSettled { game_ids }

QueryMsg::GetConfig {}
QueryMsg::GetGame { game_id }
QueryMsg::ListGames { status_filter }
```

## Configuration

Instantiation sets all table rules:

| Parameter | Description | Example |
|-----------|-------------|---------|
| `denom` | Token denomination | `uxion` |
| `min_bet` / `max_bet` | Bet limits | `1000` / `100000` |
| `blackjack_payout` | BJ payout ratio | `3:2` |
| `dealer_hits_soft_17` | Soft 17 rule | `true` |
| `dealer_peeks` | Peek for BJ | `true` |
| `double_restriction` | Double down rule | `Any` / `Hard9_10_11` / `Hard10_11` |
| `max_splits` | Max split hands | `3` |
| `shuffle_vk_id` / `reveal_vk_id` | ZK verification key IDs on Xion | `shuffle_encrypt` / `decrypt` |
| `timeout_seconds` | Inactivity timeout | `3600` |

## Prerequisites

- ZK verification keys (`shuffle_encrypt`, `decrypt`) must be registered on Xion's ZK module before the contract can verify proofs.

## Build

```bash
# Check
cargo check -p juodzekas

# Optimized wasm (requires Docker)
docker run --rm -v "$(pwd)":/code \
  --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
  --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
  cosmwasm/optimizer:0.17.0

# Output: artifacts/juodzekas.wasm
```

## Deploy

```bash
# Upload
xiond tx wasm store artifacts/juodzekas.wasm --from dealer --gas auto

# Instantiate (use code_id from upload)
xiond tx wasm instantiate <code_id> \
  '{"denom":"uxion","min_bet":"1000","max_bet":"100000",...}' \
  --label "juodzekas-blackjack" --admin <addr> --from dealer
```

Or via the Xion MCP tools if available.

## Test

```bash
# Unit + integration tests (mock ZK, fast)
cargo test -p juodzekas

# Integration with real ZK proofs (~22s, needs circuit artifacts)
cargo test -p juodzekas --test integration_real_zk

# Testnet integration (~2min, needs .env with funded wallets + deployed contract)
cargo test -p juodzekas --test testnet_zk -- --nocapture
```

## Dependencies

- `cosmwasm-std` 3.0.2 (stargate, cosmwasm_2_0)
- `cw-storage-plus` 3.0.1
- `xion-types` (burnt-labs, for ZK query types)
- `blackjack` (workspace, shared rules)
- `prost` 0.13 (protobuf for stargate queries)

## Source Layout

```
src/
  lib.rs              Entry points (feature-gated)
  msg.rs              Message types
  state.rs            Storage types (Config, GameSession, GameStatus)
  error.rs            Error types
  game_logic.rs       Bridge to blackjack package rules
  zk.rs               Xion ZK module verification
  contract/
    mod.rs            calculate_score, module exports
    instantiate.rs    Contract init
    execute.rs        All execute handlers
    query.rs          Query handlers
    reveal.rs         Card reveal + game state transitions
```
