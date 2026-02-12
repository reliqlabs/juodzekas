//! Adapter to convert between TUI's crypto-focused GameState and blackjack package's GameState
//!
//! TUI GameState: Contains encryption keys, shuffled deck, ZK proofs
//! Blackjack GameState: Contains pure game logic (spots, hands, dealer, rules)
//!
//! This module provides conversion functions to use blackjack package logic
//! while maintaining TUI's cryptographic operations

use blackjack::{GameState as BlackjackState, Spot, Hand, Card, TurnOwner, GamePhase};
use crate::game::GameState as TuiGameState;

impl TuiGameState {
    /// Convert TUI state to blackjack state for game logic operations
    pub fn to_blackjack_state(&self) -> BlackjackState {
        let mut spots: Vec<Spot> = Vec::new();

        for spot_idx in 0..self.num_spots {
            let mut spot = Spot::new();
            spot.hands.clear(); // Remove default hand

            // Convert each hand in this spot
            for hand_idx in 0..self.player_hands[spot_idx].len() {
                let tui_hand = &self.player_hands[spot_idx][hand_idx];

                let mut bj_hand = Hand::new();
                // Only add revealed cards
                for card in tui_hand.iter().flatten() {
                    bj_hand.add_card(*card);
                }

                // Set hand flags
                bj_hand.doubled = self.hands_doubled[spot_idx][hand_idx];
                bj_hand.stood = self.hands_stood[spot_idx][hand_idx];
                bj_hand.surrendered = self.hands_surrendered[spot_idx][hand_idx];

                spot.hands.push(bj_hand);
            }

            // Set active hand index for this spot
            if spot_idx == self.active_spot {
                spot.active_hand_index = self.active_hand_in_spot;
            }

            spots.push(spot);
        }

        // Convert dealer hand
        let mut dealer_hand: Vec<Card> = Vec::new();
        for card in self.dealer_hand.iter().flatten() {
            dealer_hand.push(*card);
        }

        // Determine current phase
        let phase = if dealer_hand.is_empty() {
            GamePhase::NotStarted
        } else if dealer_hand.len() == 1 {
            GamePhase::InitialDeal
        } else {
            // Check if player is still playing or dealer's turn
            let all_spots_done = spots.iter().all(|s| s.all_hands_finished());
            if all_spots_done {
                GamePhase::DealerTurn
            } else {
                GamePhase::PlayerTurn
            }
        };

        let current_turn = match phase {
            GamePhase::PlayerTurn => TurnOwner::Player,
            GamePhase::DealerTurn => TurnOwner::Dealer,
            _ => TurnOwner::None,
        };

        BlackjackState {
            spots,
            dealer_hand,
            active_spot_index: self.active_spot,
            phase,
            current_turn,
            dealer_peeked: self.dealer_peeked,
            rules: self.rules,
            last_action_timestamp: None, // TUI doesn't track timestamps
        }
    }

    /// Use blackjack package logic to check if dealer should hit
    pub fn should_dealer_hit(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        bj_state.dealer_should_hit()
    }

    /// Use blackjack package logic to check if dealer should peek for blackjack
    pub fn should_dealer_peek(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        bj_state.should_dealer_peek()
    }

    /// Use blackjack package logic to check if dealer has blackjack
    pub fn dealer_has_blackjack(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        bj_state.dealer_has_blackjack()
    }

    /// Use blackjack package logic to check if current hand can be doubled
    pub fn can_double_current_hand(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        bj_state.can_double_current_hand()
    }

    /// Use blackjack package logic to check if current hand can be surrendered
    pub fn can_surrender_current_hand(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        bj_state.can_surrender_current_hand()
    }

    /// Use blackjack package logic to check if current hand can be split
    pub fn can_split_current_hand(&self) -> bool {
        let bj_state = self.to_blackjack_state();
        let spot = bj_state.active_spot();
        spot.can_split(&self.rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameMode;

    #[test]
    fn test_to_blackjack_state() {
        let tui_state = TuiGameState::new(GameMode::Fast, 2).unwrap();
        let bj_state = tui_state.to_blackjack_state();

        assert_eq!(bj_state.spots.len(), 2);
        assert_eq!(bj_state.active_spot_index, 0);
        assert_eq!(bj_state.phase, GamePhase::NotStarted);
    }
}
