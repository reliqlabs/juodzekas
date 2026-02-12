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
    let rules = GameRules {
        max_splits: 1,
        ..GameRules::default()
    };
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
    let rules = GameRules {
        resplit_aces: false,
        ..GameRules::default()
    };
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
    let rules = GameRules {
        double_after_split: false,
        ..GameRules::default()
    };
    let mut game = GameState::new(1, rules).unwrap();

    let rules_info = game.rules;
    {
        let spot = game.active_spot_mut();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules_info).unwrap();
        spot.hands[0].cards.push(Card::TwoClubs);
        spot.hands[1].cards.push(Card::ThreeClubs);
    }

    assert!(!game.can_double_current_hand()); // Can't double after split
}

#[test]
fn test_can_double_after_split_when_enabled() {
    let rules = GameRules {
        double_after_split: true,
        ..GameRules::default()
    };
    let mut game = GameState::new(1, rules).unwrap();

    let rules_info = game.rules;
    {
        let spot = game.active_spot_mut();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules_info).unwrap();
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
    let rules = GameRules {
        dealer_hits_soft_17: true,
        ..GameRules::default()
    };
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
    let rules = GameRules {
        allow_surrender: true,
        ..GameRules::default()
    };
    let mut game = GameState::new(1, rules).unwrap();

    {
        let spot = game.active_spot_mut();
        spot.active_hand_mut().add_card(Card::TenHearts);
        spot.active_hand_mut().add_card(Card::SixSpades);
    }

    assert!(game.can_surrender_current_hand());

    game.active_spot_mut()
        .active_hand_mut()
        .add_card(Card::TwoClubs);
    assert!(!game.can_surrender_current_hand()); // 3 cards
}

#[test]
fn test_cannot_surrender_after_split() {
    let rules = GameRules {
        allow_surrender: true,
        ..GameRules::default()
    };
    let mut game = GameState::new(1, rules).unwrap();

    let rules_info = game.rules;
    {
        let spot = game.active_spot_mut();
        spot.active_hand_mut().add_card(Card::EightHearts);
        spot.active_hand_mut().add_card(Card::EightSpades);
        spot.split(&rules_info).unwrap();
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
