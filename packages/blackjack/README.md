# blackjack

Pure Rust blackjack rules engine. No blockchain or crypto dependencies -- just game logic.

## What It Does

Provides a complete blackjack state machine and rules evaluation:

- **Card representation** - 52-card enum with unicode suit display
- **Hand scoring** - Soft/hard totals, ace handling, bust detection, blackjack detection
- **Game state machine** - Full round lifecycle: deal, player actions, dealer play, settlement
- **Multi-spot play** - 1-8 simultaneous hands per player
- **Configurable rules** - Payout ratios, double restrictions, split rules, soft 17, surrender
- **Basic strategy advisor** - `optimal_move()` returns the mathematically optimal play

## API

```rust
use blackjack::{Card, GameRules, GameState, PayoutRatio, DoubleRestriction};

let rules = GameRules {
    num_decks: 6,
    blackjack_payout: PayoutRatio { numerator: 3, denominator: 2 },
    dealer_hits_soft_17: true,
    dealer_peeks: true,
    double_restriction: DoubleRestriction::Any,
    max_splits: 3,
    can_split_aces: true,
    can_hit_split_aces: false,
    surrender_allowed: true,
};

let mut game = GameState::new(rules);

// Strategy advisor
use blackjack::strategy::optimal_move;
let action = optimal_move(&player_hand, dealer_upcard, &rules);
```

## Used By

- **juodzekas contract** (`game_logic.rs`) - validates player actions (can split? can double? can surrender?) against on-chain game state
- **juodzekas-tui** - renders game, checks available actions, highlights optimal moves

## Dependencies

`serde` 1.0 only. No other dependencies.

## Source Layout

```
src/
  lib.rs          Module exports
  card.rs         Card enum (52 variants), suit/rank display
  hand.rs         Hand scoring (calculate_hand_value, is_soft, is_busted, is_blackjack)
  rules.rs        GameRules, PayoutRatio, DoubleRestriction
  game_state.rs   GameState machine, Spot, multi-hand logic, dealer play, settlement
  strategy.rs     Basic strategy advisor (optimal_move)
```
