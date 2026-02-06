use serde::{Deserialize, Serialize};
use crate::{Card, Hand, GameRules};

/// Tracks whose turn it is in the game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnOwner {
    Player,
    Dealer,
    None, // Game not started or finished
}

/// Current phase of the game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    NotStarted,
    InitialDeal,
    PlayerTurn,
    DealerTurn,
    Settled,
}

/// Represents a single spot at the table (can have multiple hands if split)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spot {
    pub hands: Vec<Hand>,
    pub active_hand_index: usize,
}

impl Spot {
    pub fn new() -> Self {
        Self {
            hands: vec![Hand::new()],
            active_hand_index: 0,
        }
    }

    pub fn active_hand(&self) -> &Hand {
        &self.hands[self.active_hand_index]
    }

    pub fn active_hand_mut(&mut self) -> &mut Hand {
        &mut self.hands[self.active_hand_index]
    }

    pub fn has_next_hand(&self) -> bool {
        self.active_hand_index + 1 < self.hands.len()
    }

    pub fn move_to_next_hand(&mut self) -> bool {
        if self.has_next_hand() {
            self.active_hand_index += 1;
            true
        } else {
            false
        }
    }

    pub fn can_split(&self, rules: &GameRules) -> bool {
        // Check if already split maximum times
        if self.hands.len() > rules.max_splits as usize {
            return false;
        }

        // Check if can split first hand
        if self.hands.len() == 1 && self.active_hand().can_split() {
            // Check if splitting aces and resplit_aces is disabled
            if let Some(first_card) = self.hands[0].cards.first() {
                if first_card.rank() == 1 && self.hands.len() > 1 && !rules.resplit_aces {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn split(&mut self, rules: &GameRules) -> Result<(), &'static str> {
        if !self.can_split(rules) {
            return Err("Cannot split");
        }

        let hand = &mut self.hands[0];
        if hand.cards.len() != 2 {
            return Err("Hand must have exactly 2 cards to split");
        }

        let second_card = hand.cards.pop().ok_or("No second card")?;

        let mut new_hand = Hand::new();
        new_hand.add_card(second_card);
        self.hands.push(new_hand);

        Ok(())
    }

    pub fn all_hands_finished(&self) -> bool {
        self.hands.iter().all(|h| h.stood || h.is_busted() || h.surrendered)
    }
}

impl Default for Spot {
    fn default() -> Self {
        Self::new()
    }
}

/// Core game state that can be shared between TUI and smart contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub spots: Vec<Spot>,
    pub dealer_hand: Vec<Card>,
    pub active_spot_index: usize,
    pub phase: GamePhase,
    pub current_turn: TurnOwner,
    pub dealer_peeked: bool,
    pub rules: GameRules,
    pub last_action_timestamp: Option<u64>, // For timeout tracking
}

impl GameState {
    pub fn new(num_spots: usize, rules: GameRules) -> Result<Self, &'static str> {
        if num_spots == 0 || num_spots > 8 {
            return Err("Number of spots must be between 1 and 8");
        }

        Ok(Self {
            spots: vec![Spot::new(); num_spots],
            dealer_hand: Vec::new(),
            active_spot_index: 0,
            phase: GamePhase::NotStarted,
            current_turn: TurnOwner::None,
            dealer_peeked: false,
            rules,
            last_action_timestamp: None,
        })
    }

    pub fn active_spot(&self) -> &Spot {
        &self.spots[self.active_spot_index]
    }

    pub fn active_spot_mut(&mut self) -> &mut Spot {
        &mut self.spots[self.active_spot_index]
    }

    pub fn dealer_value(&self) -> u8 {
        crate::calculate_hand_value(&self.dealer_hand)
    }

    pub fn dealer_should_hit(&self) -> bool {
        let value = self.dealer_value();
        if value >= 17 {
            // Check for soft 17
            if value == 17 && self.rules.dealer_hits_soft_17 {
                crate::is_soft_hand(&self.dealer_hand)
            } else {
                false
            }
        } else {
            true
        }
    }

    pub fn should_dealer_peek(&self) -> bool {
        if !self.rules.dealer_peeks || self.dealer_peeked || self.dealer_hand.is_empty() {
            return false;
        }
        // Peek if dealer shows Ace or 10-value card
        let up_card = &self.dealer_hand[0];
        let value = up_card.value();
        value == 11 || value == 10
    }

    pub fn dealer_has_blackjack(&self) -> bool {
        crate::is_blackjack(&self.dealer_hand)
    }

    pub fn move_to_next_spot(&mut self) -> bool {
        self.active_spot_index += 1;
        if self.active_spot_index < self.spots.len() {
            self.spots[self.active_spot_index].active_hand_index = 0;
            true
        } else {
            false
        }
    }

    pub fn start_player_turn(&mut self, timestamp: Option<u64>) {
        self.phase = GamePhase::PlayerTurn;
        self.current_turn = TurnOwner::Player;
        self.last_action_timestamp = timestamp;
    }

    pub fn start_dealer_turn(&mut self, timestamp: Option<u64>) {
        self.phase = GamePhase::DealerTurn;
        self.current_turn = TurnOwner::Dealer;
        self.last_action_timestamp = timestamp;
    }

    pub fn settle(&mut self) {
        self.phase = GamePhase::Settled;
        self.current_turn = TurnOwner::None;
        self.last_action_timestamp = None;
    }

    pub fn update_action_timestamp(&mut self, timestamp: u64) {
        self.last_action_timestamp = Some(timestamp);
    }

    /// Check if a timeout has occurred (for disconnect handling)
    pub fn is_timed_out(&self, current_timestamp: u64, timeout_seconds: u64) -> bool {
        if let Some(last_action) = self.last_action_timestamp {
            current_timestamp.saturating_sub(last_action) > timeout_seconds
        } else {
            false
        }
    }

    /// Get whose turn it is for timeout recovery
    pub fn get_timeout_beneficiary(&self) -> TurnOwner {
        match self.current_turn {
            TurnOwner::Player => TurnOwner::Dealer, // If player timed out, dealer wins
            TurnOwner::Dealer => TurnOwner::Player, // If dealer timed out, player wins
            TurnOwner::None => TurnOwner::None,
        }
    }

    pub fn can_double_current_hand(&self) -> bool {
        let spot = self.active_spot();
        let hand = spot.active_hand();

        // Can't double if hand has already been doubled or stood
        if hand.doubled || hand.stood {
            return false;
        }

        // Must have exactly 2 cards
        if hand.cards.len() != 2 {
            return false;
        }

        // Check if doubling after split is allowed
        if spot.hands.len() > 1 && !self.rules.double_after_split {
            return false;
        }

        // Check hand value against double restriction
        let hand_value = crate::calculate_hand_value(&hand.cards);
        let is_soft = crate::is_soft_hand(&hand.cards);

        match self.rules.double_restriction {
            crate::DoubleRestriction::Any => true,
            crate::DoubleRestriction::Hard9_10_11 => {
                !is_soft && (9..=11).contains(&hand_value)
            }
            crate::DoubleRestriction::Hard10_11 => {
                !is_soft && (10..=11).contains(&hand_value)
            }
        }
    }

    pub fn can_surrender_current_hand(&self) -> bool {
        let spot = self.active_spot();
        if spot.hands.len() > 1 {
            return false; // Can't surrender after split
        }
        let hand = spot.active_hand();
        self.rules.allow_surrender
            && hand.cards.len() == 2
            && !hand.doubled
            && !hand.stood
            && !hand.surrendered
    }

    pub fn can_split_current_hand(&self) -> bool {
        self.active_spot().can_split(&self.rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Card;

    #[test]
    fn test_game_state_new() {
        let rules = GameRules::default();
        let game = GameState::new(3, rules).unwrap();
        assert_eq!(game.spots.len(), 3);
        assert_eq!(game.active_spot_index, 0);
        assert_eq!(game.phase, GamePhase::NotStarted);
        assert_eq!(game.current_turn, TurnOwner::None);
    }

    #[test]
    fn test_game_state_new_invalid_spots() {
        let rules = GameRules::default();
        assert!(GameState::new(0, rules).is_err());
        assert!(GameState::new(9, rules).is_err());
    }

    #[test]
    fn test_spot_can_split() {
        let rules = GameRules::default();
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        assert!(spot.can_split(&rules));
    }

    #[test]
    fn test_spot_cannot_split_after_split() {
        let rules = GameRules::default();
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules).unwrap();
        assert!(!spot.can_split(&rules)); // Already split
    }

    #[test]
    fn test_spot_split() {
        let rules = GameRules::default();
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);

        spot.split(&rules).unwrap();

        assert_eq!(spot.hands.len(), 2);
        assert_eq!(spot.hands[0].cards.len(), 1);
        assert_eq!(spot.hands[1].cards.len(), 1);
        assert_eq!(spot.hands[0].cards[0], Card::EightHearts);
        assert_eq!(spot.hands[1].cards[0], Card::EightSpades);
    }

    #[test]
    fn test_spot_cannot_split_max_splits_reached() {
        let mut rules = GameRules::default();
        rules.max_splits = 1;
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules).unwrap();

        // Now we have 2 hands, max_splits is 1, so can't split again
        spot.hands[0].cards.push(Card::EightClubs);
        assert!(!spot.can_split(&rules));
    }

    #[test]
    fn test_spot_cannot_resplit_aces() {
        let mut rules = GameRules::default();
        rules.resplit_aces = false;
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::AceHearts);
        spot.active_hand_mut().add_card(Card::AceSpades);

        // First split should work
        assert!(spot.can_split(&rules));
    }

    #[test]
    fn test_spot_move_to_next_hand() {
        let rules = GameRules::default();
        let mut spot = Spot::new();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules).unwrap();

        assert_eq!(spot.active_hand_index, 0);
        assert!(spot.move_to_next_hand());
        assert_eq!(spot.active_hand_index, 1);
        assert!(!spot.move_to_next_hand()); // No more hands
    }

    #[test]
    fn test_cannot_double_after_split_when_disabled() {
        let mut rules = GameRules::default();
        rules.double_after_split = false;
        let mut game = GameState::new(1, rules).unwrap();

        let rules_clone = game.rules.clone();
        {
            let spot = game.active_spot_mut();
            spot.active_hand_mut().add_card(Card::EightHearts);
            spot.active_hand_mut().add_card(Card::EightSpades);
            spot.split(&rules_clone).unwrap();
            spot.hands[0].cards.push(Card::TwoClubs);
            spot.hands[1].cards.push(Card::ThreeClubs);
        }

        assert!(!game.can_double_current_hand()); // Can't double after split
    }

    #[test]
    fn test_can_double_after_split_when_enabled() {
        let mut rules = GameRules::default();
        rules.double_after_split = true;
        let mut game = GameState::new(1, rules).unwrap();

        let rules_clone = game.rules.clone();
        {
            let spot = game.active_spot_mut();
            spot.active_hand_mut().add_card(Card::EightHearts);
            spot.active_hand_mut().add_card(Card::EightSpades);
            spot.split(&rules_clone).unwrap();
            spot.hands[0].cards.push(Card::TwoClubs);
            spot.hands[1].cards.push(Card::ThreeClubs);
        }

        assert!(game.can_double_current_hand()); // Can double after split
    }

    #[test]
    fn test_dealer_should_hit() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.dealer_hand.push(Card::TenHearts);
        game.dealer_hand.push(Card::SixSpades);
        assert!(game.dealer_should_hit()); // 16

        game.dealer_hand.push(Card::FiveClubs);
        assert!(!game.dealer_should_hit()); // 21
    }

    #[test]
    fn test_dealer_should_hit_soft_17() {
        let mut rules = GameRules::default();
        rules.dealer_hits_soft_17 = true;
        let mut game = GameState::new(1, rules).unwrap();

        game.dealer_hand.push(Card::AceHearts);
        game.dealer_hand.push(Card::SixSpades);
        assert!(game.dealer_should_hit()); // Soft 17
    }

    #[test]
    fn test_dealer_should_not_hit_hard_17() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.dealer_hand.push(Card::TenHearts);
        game.dealer_hand.push(Card::SevenSpades);
        assert!(!game.dealer_should_hit()); // Hard 17
    }

    #[test]
    fn test_should_dealer_peek() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.dealer_hand.push(Card::AceHearts);
        assert!(game.should_dealer_peek()); // Ace showing

        game.dealer_peeked = true;
        assert!(!game.should_dealer_peek()); // Already peeked
    }

    #[test]
    fn test_dealer_has_blackjack() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.dealer_hand.push(Card::AceHearts);
        game.dealer_hand.push(Card::KingSpades);
        assert!(game.dealer_has_blackjack());

        game.dealer_hand.push(Card::TwoClubs);
        assert!(!game.dealer_has_blackjack()); // 3 cards, not blackjack
    }

    #[test]
    fn test_move_to_next_spot() {
        let rules = GameRules::default();
        let mut game = GameState::new(3, rules).unwrap();

        assert_eq!(game.active_spot_index, 0);
        assert!(game.move_to_next_spot());
        assert_eq!(game.active_spot_index, 1);
        assert!(game.move_to_next_spot());
        assert_eq!(game.active_spot_index, 2);
        assert!(!game.move_to_next_spot()); // No more spots
    }

    #[test]
    fn test_turn_transitions() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        assert_eq!(game.phase, GamePhase::NotStarted);
        assert_eq!(game.current_turn, TurnOwner::None);

        game.start_player_turn(Some(100));
        assert_eq!(game.phase, GamePhase::PlayerTurn);
        assert_eq!(game.current_turn, TurnOwner::Player);
        assert_eq!(game.last_action_timestamp, Some(100));

        game.start_dealer_turn(Some(200));
        assert_eq!(game.phase, GamePhase::DealerTurn);
        assert_eq!(game.current_turn, TurnOwner::Dealer);
        assert_eq!(game.last_action_timestamp, Some(200));

        game.settle();
        assert_eq!(game.phase, GamePhase::Settled);
        assert_eq!(game.current_turn, TurnOwner::None);
        assert_eq!(game.last_action_timestamp, None);
    }

    #[test]
    fn test_timeout_detection() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.start_player_turn(Some(100));

        assert!(!game.is_timed_out(200, 300)); // 100s elapsed, 300s timeout
        assert!(game.is_timed_out(500, 300)); // 400s elapsed, 300s timeout
    }

    #[test]
    fn test_timeout_beneficiary() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        game.start_player_turn(Some(100));
        assert_eq!(game.get_timeout_beneficiary(), TurnOwner::Dealer);

        game.start_dealer_turn(Some(200));
        assert_eq!(game.get_timeout_beneficiary(), TurnOwner::Player);

        game.settle();
        assert_eq!(game.get_timeout_beneficiary(), TurnOwner::None);
    }

    #[test]
    fn test_can_double_current_hand() {
        let rules = GameRules::default();
        let mut game = GameState::new(1, rules).unwrap();

        {
            let spot = game.active_spot_mut();
            spot.active_hand_mut().add_card(Card::TenHearts);
            spot.active_hand_mut().add_card(Card::SevenSpades);
        }

        assert!(game.can_double_current_hand());

        game.active_spot_mut().active_hand_mut().doubled = true;
        assert!(!game.can_double_current_hand());
    }

    #[test]
    fn test_can_surrender_current_hand() {
        let mut rules = GameRules::default();
        rules.allow_surrender = true;
        let mut game = GameState::new(1, rules).unwrap();

        {
            let spot = game.active_spot_mut();
            spot.active_hand_mut().add_card(Card::TenHearts);
            spot.active_hand_mut().add_card(Card::SixSpades);
        }

        assert!(game.can_surrender_current_hand());

        game.active_spot_mut().active_hand_mut().add_card(Card::TwoClubs);
        assert!(!game.can_surrender_current_hand()); // 3 cards
    }

    #[test]
    fn test_cannot_surrender_after_split() {
        let mut rules = GameRules::default();
        rules.allow_surrender = true;
        let mut game = GameState::new(1, rules).unwrap();

        let rules_clone = game.rules.clone();
        {
            let spot = game.active_spot_mut();
            spot.active_hand_mut().add_card(Card::EightHearts);
            spot.active_hand_mut().add_card(Card::EightSpades);
            spot.split(&rules_clone).unwrap();
        }

        assert!(!game.can_surrender_current_hand()); // Can't surrender after split
    }

    #[test]
    fn test_payout_calculation() {
        let rules = GameRules::default();
        assert_eq!(rules.blackjack_payout.calculate_payout(100), 150);

        let rules_six_five = GameRules::single_deck();
        assert_eq!(rules_six_five.blackjack_payout.calculate_payout(100), 120);
    }
}
