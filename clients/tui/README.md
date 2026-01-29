# Juodžekas Terminal UI

This is the terminal user interface for the Juodžekas trustless Blackjack game. It is built using the `ratatui` library and provides an interactive way to play the game directly from your terminal.

## How to Run

**IMPORTANT:** You must run from the workspace root directory:

```bash
cd /path/to/juodzekas  # The root directory
cargo run -p juodzekas-tui
```

**Do not** run from the `clients/tui/` directory, as the game needs access to circuit files in `circuits/artifacts/`.

## Game Modes

When you start the game, you'll be asked to choose a mode:

- **Fast Mode ([F])**: Instant gameplay with cryptographic shuffle but no ZK proofs (~1 second to start)
- **Trustless Mode ([T])**: Full ZK proof generation and verification (~1 minute to start)
  - Uses WASM witness calculator + rapidsnark for reliable proof generation
  - Generates and verifies shuffle proofs: ~30 seconds per shuffle (player + dealer = ~1 minute)
  - Completely trustless - cryptographic proof that no one cheated

### Performance Tuning (Apple Silicon)

On Apple Silicon Macs (M1/M2/M3), you can improve proof generation performance by limiting Rayon to Performance cores only:

```bash
# M2 Air (4 P-cores + 4 E-cores) - use only P-cores
RAYON_NUM_THREADS=4 cargo run -p juodzekas-tui

# M2 Pro/Max (8 P-cores) - use only P-cores
RAYON_NUM_THREADS=8 cargo run -p juodzekas-tui
```

This prevents slower Efficiency cores from bottlenecking parallel proof generation. Test with different thread counts (4, 6, 8) to find optimal performance for your hardware.

## Layout

The TUI is organized into several sections:

```
┌─────────────────────────────────────────────────────────────────────┐
│                  Juodžekas - Trustless Blackjack                    │
├─────────────────────────────────────┬───────────────────────────────┤
│        Dealer Hand │ Player Hand    │   ┌───────────┐               │
│         7♦  ??     │    A♠  J♥      │   │ J         │               │
│                    │                │   │  _____    │               │
│                    │                │   │ /     \   │   The Jack    │
│                    │                │   │ | O O |   │               │
│                    │                │   │ |  ^  |   │               │
│                    │                │   │ | \_/ |   │               │
│                    │                │   │  \___/    │               │
│                    │                │   │         J │               │
│                    │                │   └───────────┘               │
│                    │                ├───────────────────────────────┤
│                    │                │        Game Log               │
│                    │                │  • Welcome to Juodžekas!      │
│                    │                │  • Connected to game...       │
│                    │                │  • Shuffling deck...          │
├────────────────────────────────────┴───────────────────────────────┤
│ Your Turn: [H]it, [S]tand, [D]ouble, [P]lit, [R]urrender           │
└─────────────────────────────────────────────────────────────────────┘
```

### Components

- **Title Bar**: Game name and title
- **Dealer Hand**: Top panel showing dealer's visible cards (second card hidden during player turn)
- **Player Hand**: Bottom panel showing your cards with total value
- **Game Log**: Right panel showing scrolling log of game events and actions
- **Status Bar**: Current action prompt and available controls

## Controls

| Key | Action |
|-----|--------|
| `q` | Quit the game |
| `h` | Hit (Ask for another card) |
| `s` | Stand (Keep your current hand) |
| `d` | Double Down (Double bet, take one card) |
| `p` | Split (Split a pair into two hands) |
| `r` | Surrender (Forfeit half your bet) |

## Requirements

- A terminal that supports ANSI escape sequences and true color (most modern terminals).
- Minimum terminal size: 80x24 characters.
