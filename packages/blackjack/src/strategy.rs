use crate::{Card, GameRules};
use crate::hand::{calculate_hand_value, is_soft_hand, can_split_cards};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimalMove {
    Hit,
    Stand,
    Double,
    Split,
    Surrender,
}

/// Get the optimal move based on basic strategy
pub fn optimal_move(
    player_cards: &[Card],
    dealer_up_card: &Card,
    can_double: bool,
    can_split: bool,
    can_surrender: bool,
    rules: &GameRules,
) -> OptimalMove {
    let player_value = calculate_hand_value(player_cards);
    let dealer_value = dealer_up_card.value();
    let is_soft = is_soft_hand(player_cards);

    // Check for surrender (before split/double)
    if can_surrender && rules.allow_surrender {
        if !is_soft {
            if player_value == 16 && (dealer_value == 9 || dealer_value == 10 || dealer_value == 11) {
                return OptimalMove::Surrender;
            }
            if player_value == 15 && dealer_value == 10 {
                return OptimalMove::Surrender;
            }
        }
    }

    // Check if can split
    if can_split && player_cards.len() == 2 && can_split_cards(&player_cards[0], &player_cards[1]) {
        let card_rank = player_cards[0].rank();

        // Always split Aces and 8s
        if card_rank == 1 || card_rank == 8 {
            return OptimalMove::Split;
        }
        // Never split 10s, 5s, 4s
        if card_rank == 10 || card_rank == 11 || card_rank == 12 || card_rank == 13 || card_rank == 5 || card_rank == 4 {
            // Fall through to regular strategy
        } else if card_rank == 9 {
            // Split 9s except against 7, 10, or Ace
            if dealer_value != 7 && dealer_value != 10 && dealer_value != 11 {
                return OptimalMove::Split;
            }
        } else if card_rank == 7 || card_rank == 6 {
            // Split 7s and 6s against 2-7
            if dealer_value >= 2 && dealer_value <= 7 {
                return OptimalMove::Split;
            }
        } else if card_rank == 3 || card_rank == 2 {
            // Split 2s and 3s against 2-7
            if dealer_value >= 2 && dealer_value <= 7 {
                return OptimalMove::Split;
            }
        }
    }

    // Check if can double
    if can_double {
        if is_soft {
            // Soft doubling
            if player_value == 19 && dealer_value == 6 {
                return OptimalMove::Double;
            } else if player_value == 18 && dealer_value >= 2 && dealer_value <= 6 {
                return OptimalMove::Double;
            } else if player_value == 17 && dealer_value >= 3 && dealer_value <= 6 {
                return OptimalMove::Double;
            } else if player_value >= 15 && player_value <= 16 && dealer_value >= 4 && dealer_value <= 6 {
                return OptimalMove::Double;
            } else if player_value >= 13 && player_value <= 14 && dealer_value >= 5 && dealer_value <= 6 {
                return OptimalMove::Double;
            }
        } else {
            // Hard doubling
            if player_value == 11 {
                return OptimalMove::Double;
            } else if player_value == 10 && dealer_value <= 9 {
                return OptimalMove::Double;
            } else if player_value == 9 && dealer_value >= 3 && dealer_value <= 6 {
                return OptimalMove::Double;
            }
        }
    }

    // Basic strategy for hitting/standing
    if is_soft {
        // Soft hands
        if player_value >= 19 {
            OptimalMove::Stand
        } else if player_value == 18 {
            if dealer_value >= 9 {
                OptimalMove::Hit
            } else {
                OptimalMove::Stand
            }
        } else {
            OptimalMove::Hit
        }
    } else {
        // Hard hands
        if player_value >= 17 {
            OptimalMove::Stand
        } else if player_value >= 13 && player_value <= 16 {
            if dealer_value >= 2 && dealer_value <= 6 {
                OptimalMove::Stand
            } else {
                OptimalMove::Hit
            }
        } else if player_value == 12 {
            if dealer_value >= 4 && dealer_value <= 6 {
                OptimalMove::Stand
            } else {
                OptimalMove::Hit
            }
        } else {
            OptimalMove::Hit
        }
    }
}
