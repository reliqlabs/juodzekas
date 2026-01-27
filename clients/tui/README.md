# Juodžekas Terminal UI

This is the terminal user interface for the Juodžekas trustless Blackjack game. It is built using the `ratatui` library and provides an interactive way to play the game directly from your terminal.

## How to Run

From the root of the workspace, run:

```bash
cargo run -p juodzekas-tui
```

Alternatively, from this directory:

```bash
cargo run
```

## Features (Mocked)

Currently, the TUI is a scaffold with mocked game state to demonstrate the layout:
- **Dealer Hand**: Displays the dealer's cards and current score.
- **Player Hand**: Displays your cards and current score.
- **Actions Bar**: Shows available moves and their respective keys.
- **Status Log**: Displays messages about the game flow.

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
