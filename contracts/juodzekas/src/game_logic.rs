use blackjack::{Card, GameRules, GameState, Hand as BjHand, PayoutRatio, Spot, TurnOwner as BjTurnOwner, GamePhase};
use crate::state::{GameSession, HandStatus, GameStatus, TurnOwner, Config};

/// Convert contract GameSession to blackjack GameState for rule validation
pub fn to_blackjack_state(session: &GameSession, rules: GameRules) -> GameState {
    // Convert hands
    let mut spots = Vec::new();
    let mut spot = Spot::new();
    spot.hands.clear();

    for hand in &session.hands {
        let mut bj_hand = BjHand::new();
        for &card_value in &hand.cards {
            bj_hand.add_card(card_value_to_card(card_value));
        }

        match hand.status {
            HandStatus::Doubled => bj_hand.doubled = true,
            HandStatus::Stood => bj_hand.stood = true,
            HandStatus::Surrendered => bj_hand.surrendered = true,
            _ => {}
        }

        spot.hands.push(bj_hand);
    }

    if session.current_hand_index < session.hands.len() as u32 {
        spot.active_hand_index = session.current_hand_index as usize;
    }

    spots.push(spot);

    // Convert dealer hand
    let dealer_cards: Vec<Card> = session.dealer_hand.iter()
        .map(|&v| card_value_to_card(v))
        .collect();

    // Determine phase
    let phase = match &session.status {
        GameStatus::WaitingForPlayerJoin | GameStatus::WaitingForDealerJoin => GamePhase::NotStarted,
        GameStatus::WaitingForPlayerShuffle | GameStatus::WaitingForDealerShuffle => GamePhase::NotStarted,
        GameStatus::WaitingForReveal { .. } if session.dealer_hand.is_empty() => GamePhase::InitialDeal,
        GameStatus::WaitingForReveal { .. } => GamePhase::DealerTurn, // Card reveals during game
        GameStatus::PlayerTurn => GamePhase::PlayerTurn,
        GameStatus::DealerTurn => GamePhase::DealerTurn,
        GameStatus::Settled { .. } => GamePhase::Settled,
    };

    // Convert turn owner
    let current_turn = match session.current_turn {
        TurnOwner::Player => BjTurnOwner::Player,
        TurnOwner::Dealer => BjTurnOwner::Dealer,
        TurnOwner::None => BjTurnOwner::None,
    };

    GameState {
        spots,
        dealer_hand: dealer_cards,
        active_spot_index: 0,
        phase,
        current_turn,
        dealer_peeked: false, // Would need to track this in contract if needed
        rules,
        last_action_timestamp: Some(session.last_action_timestamp),
    }
}

/// Convert contract Config to blackjack GameRules
pub fn config_to_rules(config: &Config) -> GameRules {
    // Convert contract PayoutRatio to blackjack PayoutRatio
    let blackjack_payout = PayoutRatio::new(
        config.blackjack_payout.numerator,
        config.blackjack_payout.denominator
    ).unwrap_or(PayoutRatio::THREE_TO_TWO);

    // Convert contract DoubleRestriction to blackjack DoubleRestriction
    let double_restriction = match config.double_restriction {
        crate::state::DoubleRestriction::Any => blackjack::DoubleRestriction::Any,
        crate::state::DoubleRestriction::Hard9_10_11 => blackjack::DoubleRestriction::Hard9_10_11,
        crate::state::DoubleRestriction::Hard10_11 => blackjack::DoubleRestriction::Hard10_11,
    };

    GameRules {
        dealer_hits_soft_17: config.dealer_hits_soft_17,
        allow_surrender: config.surrender_allowed,
        late_surrender: true, // Assume late surrender if surrender allowed
        double_after_split: true, // Would need to add to Config if different
        double_restriction,
        allow_resplit: config.max_splits > 0,
        max_splits: config.max_splits as u8,
        resplit_aces: config.can_split_aces,
        dealer_peeks: config.dealer_peeks,
        blackjack_payout,
        num_decks: 1, // Would need to add to Config
    }
}

/// Convert u8 card value (0-51) to Card enum
/// Contract uses 0-51 indexing where rank = (value % 13) and suit = (value / 13)
fn card_value_to_card(value: u8) -> Card {
    let rank = value % 13;  // 0-12 (Ace through King)
    let suit = value / 13;  // 0-3 (Spades, Hearts, Diamonds, Clubs)

    match (suit, rank) {
        (0, 0) => Card::AceSpades,
        (0, 1) => Card::TwoSpades,
        (0, 2) => Card::ThreeSpades,
        (0, 3) => Card::FourSpades,
        (0, 4) => Card::FiveSpades,
        (0, 5) => Card::SixSpades,
        (0, 6) => Card::SevenSpades,
        (0, 7) => Card::EightSpades,
        (0, 8) => Card::NineSpades,
        (0, 9) => Card::TenSpades,
        (0, 10) => Card::JackSpades,
        (0, 11) => Card::QueenSpades,
        (0, 12) => Card::KingSpades,
        (1, 0) => Card::AceHearts,
        (1, 1) => Card::TwoHearts,
        (1, 2) => Card::ThreeHearts,
        (1, 3) => Card::FourHearts,
        (1, 4) => Card::FiveHearts,
        (1, 5) => Card::SixHearts,
        (1, 6) => Card::SevenHearts,
        (1, 7) => Card::EightHearts,
        (1, 8) => Card::NineHearts,
        (1, 9) => Card::TenHearts,
        (1, 10) => Card::JackHearts,
        (1, 11) => Card::QueenHearts,
        (1, 12) => Card::KingHearts,
        (2, 0) => Card::AceDiamonds,
        (2, 1) => Card::TwoDiamonds,
        (2, 2) => Card::ThreeDiamonds,
        (2, 3) => Card::FourDiamonds,
        (2, 4) => Card::FiveDiamonds,
        (2, 5) => Card::SixDiamonds,
        (2, 6) => Card::SevenDiamonds,
        (2, 7) => Card::EightDiamonds,
        (2, 8) => Card::NineDiamonds,
        (2, 9) => Card::TenDiamonds,
        (2, 10) => Card::JackDiamonds,
        (2, 11) => Card::QueenDiamonds,
        (2, 12) => Card::KingDiamonds,
        (3, 0) => Card::AceClubs,
        (3, 1) => Card::TwoClubs,
        (3, 2) => Card::ThreeClubs,
        (3, 3) => Card::FourClubs,
        (3, 4) => Card::FiveClubs,
        (3, 5) => Card::SixClubs,
        (3, 6) => Card::SevenClubs,
        (3, 7) => Card::EightClubs,
        (3, 8) => Card::NineClubs,
        (3, 9) => Card::TenClubs,
        (3, 10) => Card::JackClubs,
        (3, 11) => Card::QueenClubs,
        (3, 12) => Card::KingClubs,
        _ => Card::AceSpades, // Fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{Addr, Binary, Uint128};

    #[test]
    fn test_card_value_conversion() {
        // Contract uses 0-51 indexing
        assert_eq!(card_value_to_card(0), Card::AceSpades);   // rank 0, suit 0
        assert_eq!(card_value_to_card(3), Card::FourSpades);  // rank 3, suit 0
        assert_eq!(card_value_to_card(12), Card::KingSpades); // rank 12, suit 0
        assert_eq!(card_value_to_card(13), Card::AceHearts);  // rank 0, suit 1
        assert_eq!(card_value_to_card(25), Card::KingHearts); // rank 12, suit 1
        assert_eq!(card_value_to_card(51), Card::KingClubs);  // rank 12, suit 3
    }

    #[test]
    fn test_config_to_rules_three_to_two() {
        let config = Config {
            denom: "utoken".to_string(),
            min_bet: Uint128::new(100),
            max_bet: Uint128::new(1000),
            blackjack_payout: crate::state::PayoutRatio { numerator: 3, denominator: 2 },
            insurance_payout: crate::state::PayoutRatio { numerator: 2, denominator: 1 },
            standard_payout: crate::state::PayoutRatio { numerator: 1, denominator: 1 },
            dealer_hits_soft_17: false,
            dealer_peeks: true,
            double_restriction: crate::state::DoubleRestriction::Any,
            max_splits: 3,
            can_split_aces: false,
            can_hit_split_aces: false,
            surrender_allowed: true,
            shuffle_vk_id: "test".to_string(),
            reveal_vk_id: "test".to_string(),
        };

        let rules = config_to_rules(&config);
        assert_eq!(rules.blackjack_payout, PayoutRatio::THREE_TO_TWO);
        assert!(!rules.dealer_hits_soft_17);
        assert!(rules.dealer_peeks);
        assert!(rules.allow_surrender);
        assert_eq!(rules.max_splits, 3);
    }

    #[test]
    fn test_to_blackjack_state_basic() {
        let session = GameSession {
            player: Addr::unchecked("player"),
            dealer: Addr::unchecked("dealer"),
            bet: Uint128::new(100),
            player_pubkey: Binary::default(),
            dealer_pubkey: Binary::default(),
            deck: vec![],
            player_shuffled_deck: None,
            hands: vec![crate::state::Hand {
                cards: vec![1, 23], // Two of Spades, King of Hearts
                bet: Uint128::new(100),
                status: HandStatus::Active,
            }],
            current_hand_index: 0,
            dealer_hand: vec![10], // Jack of Spades
            status: GameStatus::PlayerTurn,
            current_turn: TurnOwner::Player,
            last_action_timestamp: 1000,
            last_card_index: 2,
            pending_reveals: vec![],
        };

        let rules = GameRules::default();
        let state = to_blackjack_state(&session, rules);

        assert_eq!(state.phase, GamePhase::PlayerTurn);
        assert_eq!(state.current_turn, BjTurnOwner::Player);
        assert_eq!(state.spots.len(), 1);
        assert_eq!(state.spots[0].hands.len(), 1);
        assert_eq!(state.spots[0].hands[0].cards.len(), 2);
        assert_eq!(state.dealer_hand.len(), 1);
        assert_eq!(state.last_action_timestamp, Some(1000));
    }
}
