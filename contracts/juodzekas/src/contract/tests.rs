use super::*;
use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
use cosmwasm_std::{coins, from_json, Addr, Binary, Uint128};
use crate::msg::{ExecuteMsg, GameResponse, InstantiateMsg, QueryMsg};
use crate::state::Config;

#[test]
fn test_calculate_score() {
    // Standard hand
    assert_eq!(calculate_score(&[0, 10]), 21); // Ace (0%13+1=1) and Jack (10%13+1=11 -> 10)
    // Two aces
    assert_eq!(calculate_score(&[0, 13]), 12); // Two Aces: 11 + 1 = 12
    // Bust and Ace adjustment
    assert_eq!(calculate_score(&[0, 9, 8]), 20); // Ace (1), 10, 9 -> 1+10+9=20.
    
    assert_eq!(calculate_score(&[0]), 11); // Ace
    assert_eq!(calculate_score(&[0, 0]), 12); // Ace, Ace
    assert_eq!(calculate_score(&[0, 9]), 21); // Ace, 10
    assert_eq!(calculate_score(&[0, 8]), 20); // Ace, 9
    assert_eq!(calculate_score(&[0, 10, 10]), 21); // Ace, 10, 10 (1+10+10)
}

#[test]
fn test_full_game_flow() {
    let mut deps = mock_dependencies();
    let creator = "cosmwasm1zg63vla7v7svzpxatp6y0v5fuv8vml5u7e66ax";
    let player = "cosmwasm1zg63vla7v7svzpxatp6y0v5fuv8vml5u7e66ax";

    // 1. Instantiate
    let inst_msg = InstantiateMsg {
        min_bet: Uint128::new(100),
        max_bet: Uint128::new(10000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 3,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "shuffle_key".to_string(),
        reveal_vk_id: "reveal_key".to_string(),
    };
    let info = message_info(&Addr::unchecked(creator), &[]);
    instantiate(deps.as_mut(), mock_env(), info, inst_msg).unwrap();

    // 2. Join Game
    let join_msg = ExecuteMsg::JoinGame {
        bet: Uint128::new(100),
        public_key: Binary::from(b"player_pk"),
    };
    let info = message_info(&Addr::unchecked(player), &[]);
    execute(deps.as_mut(), mock_env(), info.clone(), join_msg).unwrap();

    // 3. Submit Shuffle
    let shuffle_msg = ExecuteMsg::SubmitShuffle {
        shuffled_deck: vec![Binary::from(b"card0"); 52],
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), shuffle_msg).unwrap();

    // Check game status
    let game_res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetGame {
            player: player.to_string(),
        },
    ).unwrap();
    let game: GameResponse = from_json(&game_res).unwrap();
    assert_eq!(
        game.status,
        "WaitingForReveal { reveal_requests: [0, 1, 2], next_status: PlayerTurn }"
    );

    // 4. Reveal initial cards
    // Card 0: Player (0%52 = 0 -> Ace)
    let reveal_msg = ExecuteMsg::SubmitReveal {
        card_index: 0,
        partial_decryption: Binary::from(&[0]),
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), reveal_msg).unwrap();

    // Check if hand 0 has the card
    let game: GameResponse = from_json(
        &query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()
    ).unwrap();
    assert_eq!(game.hands[0].cards, vec![0]);

    // Card 1: Player (10%52 = 10 -> Jack)
    let reveal_msg = ExecuteMsg::SubmitReveal {
        card_index: 1,
        partial_decryption: Binary::from(&[10]),
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), reveal_msg).unwrap();

    // Card 2: Dealer (1%52 = 1 -> 2)
    let reveal_msg = ExecuteMsg::SubmitReveal {
        card_index: 2,
        partial_decryption: Binary::from(&[1]),
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), reveal_msg).unwrap();

    let game_status_query = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetGame {
            player: player.to_string(),
        },
    ).unwrap();
    let game: GameResponse = from_json(game_status_query).unwrap();
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [3], next_status: DealerTurn }");
    
    // Card 3: Dealer (hidden) (5%52 = 5 -> 6)
    let reveal_msg = ExecuteMsg::SubmitReveal {
        card_index: 3,
        partial_decryption: Binary::from(&[5]),
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), reveal_msg).unwrap();

    let game: GameResponse = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::GetGame {
                player: player.to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    // Dealer has 2 and 6 = 8. Should hit!
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [4], next_status: DealerTurn }");

    // Card 4: Dealer hit (9%52 = 9 -> 10)
    let reveal_msg = ExecuteMsg::SubmitReveal {
        card_index: 4,
        partial_decryption: Binary::from(&[9]),
        proof: Binary::from(b"valid_proof"),
    };
    execute(deps.as_mut(), mock_env(), info.clone(), reveal_msg).unwrap();

    let game: GameResponse = from_json(
        query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::GetGame {
                player: player.to_string(),
            },
        )
        .unwrap(),
    )
    .unwrap();

    // Dealer has 2 + 6 + 10 = 18. Should stand!
    // Player has 21. Player wins!
    assert_eq!(game.status, "Settled { winner: \"Player (Blackjack)\" }");
    assert_eq!(game.hands[0].status, "Settled { winner: \"Player (Blackjack)\" }");
}

#[test]
fn test_bet_limits() {
    let mut deps = mock_dependencies();
    let creator = "creator";
    let player = "player";

    // 1. Instantiate with min_bet=100 and max_bet=1000
    let inst_msg = InstantiateMsg {
        min_bet: Uint128::new(100),
        max_bet: Uint128::new(1000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 3,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "shuffle_key".to_string(),
        reveal_vk_id: "reveal_key".to_string(),
    };
    let info = message_info(&Addr::unchecked(creator), &[]);
    instantiate(deps.as_mut(), mock_env(), info, inst_msg).unwrap();

    // 2. Try to join with bet < min_bet (50 < 100)
    let join_msg_low = ExecuteMsg::JoinGame {
        bet: Uint128::new(50),
        public_key: Binary::from(b"player_pk"),
    };
    let info_player = message_info(&Addr::unchecked(player), &[]);
    let err = execute(deps.as_mut(), mock_env(), info_player.clone(), join_msg_low).unwrap_err();
    assert!(err.to_string().contains("Bet too low"));

    // 3. Try to join with bet > max_bet (1500 > 1000)
    let join_msg_high = ExecuteMsg::JoinGame {
        bet: Uint128::new(1500),
        public_key: Binary::from(b"player_pk"),
    };
    let err = execute(deps.as_mut(), mock_env(), info_player.clone(), join_msg_high).unwrap_err();
    assert!(err.to_string().contains("Bet too high"));

    // 4. Join with valid bet (500)
    let join_msg_valid = ExecuteMsg::JoinGame {
        bet: Uint128::new(500),
        public_key: Binary::from(b"player_pk"),
    };
    execute(deps.as_mut(), mock_env(), info_player, join_msg_valid).unwrap();
}

#[test]
fn test_dealer_soft_17() {
    // Test Case A: Dealer hits on Soft 17
    {
        let mut deps = mock_dependencies();
        let inst_msg = InstantiateMsg {
            min_bet: Uint128::new(10),
            max_bet: Uint128::new(1000),
            bj_payout_permille: 1500,
            insurance_payout_permille: 2000,
            standard_payout_permille: 1000,
            dealer_hits_soft_17: true, // HIT ON SOFT 17
            dealer_peeks: true,
            double_down_restriction: crate::state::DoubleDownRestriction::Any,
            max_splits: 3,
            can_split_aces: true,
            can_hit_split_aces: false,
            surrender_allowed: true,
            shuffle_vk_id: "shuffle_key".to_string(),
            reveal_vk_id: "reveal_key".to_string(),
        };
        instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("creator"), &[]), inst_msg).unwrap();
        
        let player = Addr::unchecked("player");
        let info = message_info(&player, &[]);
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

        // Reveal initial cards:
        // P: Card 0 (val 2), Card 1 (val 2) -> Score 4
        // D: Card 2 (val 0 -> Ace), Card 3 (val 5 -> 6) -> Score 17 (Soft 17)
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[0]), proof: Binary::from(b"valid_proof") }).unwrap();
        
        // Player stands on 4 (for testing dealer behavior)
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Stand {}).unwrap();
        
        // Reveal Dealer's hole card (index 3) -> 6
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 3, partial_decryption: Binary::from(&[5]), proof: Binary::from(b"valid_proof") }).unwrap();

        let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
        // Since dealer_hits_soft_17 is true, dealer should be WaitingForReveal for card index 4
        assert_eq!(game.status, "WaitingForReveal { reveal_requests: [4], next_status: DealerTurn }");
    }

    // Test Case B: Dealer stands on Soft 17
    {
        let mut deps = mock_dependencies();
        let inst_msg = InstantiateMsg {
            min_bet: Uint128::new(10),
            max_bet: Uint128::new(1000),
            bj_payout_permille: 1500,
            insurance_payout_permille: 2000,
            standard_payout_permille: 1000,
            dealer_hits_soft_17: false, // STAND ON SOFT 17
            dealer_peeks: true,
            double_down_restriction: crate::state::DoubleDownRestriction::Any,
            max_splits: 3,
            can_split_aces: true,
            can_hit_split_aces: false,
            surrender_allowed: true,
            shuffle_vk_id: "shuffle_key".to_string(),
            reveal_vk_id: "reveal_key".to_string(),
        };
        instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("creator"), &[]), inst_msg).unwrap();
        
        let player = Addr::unchecked("player");
        let info = message_info(&player, &[]);
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

        // Reveal initial cards:
        // P: Card 0 (val 2), Card 1 (val 2) -> Score 4
        // D: Card 2 (val 0 -> Ace), Card 3 (val 5 -> 6) -> Score 17 (Soft 17)
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[0]), proof: Binary::from(b"valid_proof") }).unwrap();
        
        // Player stands
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Stand {}).unwrap();
        
        // Reveal Dealer's hole card (index 3) -> 6
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 3, partial_decryption: Binary::from(&[5]), proof: Binary::from(b"valid_proof") }).unwrap();

        let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
        // Since dealer_hits_soft_17 is false, dealer stands on 17.
        // D(17) vs P(4) -> Dealer wins.
        assert_eq!(game.status, "Settled { winner: \"Dealer\" }");
    }
}

#[test]
fn test_double_down_restrictions() {
    use crate::state::DoubleDownRestriction;

    let restrictions = vec![
        (DoubleDownRestriction::Any, 5, true),        // Any: 5 is allowed
        (DoubleDownRestriction::Hard9_10_11, 8, false), // Hard 9-11: 8 is NOT allowed
        (DoubleDownRestriction::Hard9_10_11, 9, true),  // Hard 9-11: 9 IS allowed
        (DoubleDownRestriction::Hard10_11, 9, false),   // Hard 10-11: 9 is NOT allowed
        (DoubleDownRestriction::Hard10_11, 10, true),   // Hard 10-11: 10 IS allowed
    ];

    for (restriction, player_total, should_allow) in restrictions {
        let mut deps = mock_dependencies();
        let inst_msg = InstantiateMsg {
            min_bet: Uint128::new(10),
            max_bet: Uint128::new(1000),
            bj_payout_permille: 1500,
            insurance_payout_permille: 2000,
            standard_payout_permille: 1000,
            dealer_hits_soft_17: true,
            dealer_peeks: true,
            double_down_restriction: restriction,
            max_splits: 3,
            can_split_aces: true,
            can_hit_split_aces: false,
            surrender_allowed: true,
            shuffle_vk_id: "s".to_string(),
            reveal_vk_id: "r".to_string(),
        };
        instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("c"), &[]), inst_msg).unwrap();
        
        let player = Addr::unchecked("p");
        let info = message_info(&player, &[]);
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

        // Setup hand for player to reach player_total
        let card1_idx = (player_total / 2) - 1;
        let card2_idx = (player_total - (player_total / 2)) - 1;

        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[card1_idx as u8]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[card2_idx as u8]), proof: Binary::from(b"valid_proof") }).unwrap();
        execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[10]), proof: Binary::from(b"valid_proof") }).unwrap(); // Dealer 2

        let res = execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::DoubleDown {});
        if should_allow {
            assert!(res.is_ok(), "Should allow for total {}, got error: {:?}", player_total, res.err());
            
            let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
            assert_eq!(game.hands[0].bet, Uint128::new(200));
            assert_eq!(game.hands[0].status, "Doubled");
        } else {
            assert!(res.is_err());
            assert!(res.unwrap_err().to_string().contains("Double down not allowed"));
        }
    }
}

#[test]
fn test_blackjack_payout() {
    let mut deps = mock_dependencies();
    // 3:2 payout (1500 permille)
    let inst_msg = InstantiateMsg {
        min_bet: Uint128::new(10),
        max_bet: Uint128::new(1000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 3,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "s".to_string(),
        reveal_vk_id: "r".to_string(),
    };
    instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("c"), &[]), inst_msg).unwrap();
    
    let player = Addr::unchecked("p");
    let info = message_info(&player, &[]);
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

    // P: Ace (0), Ten (9) -> Blackjack
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[0]), proof: Binary::from(b"valid_proof") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[9]), proof: Binary::from(b"valid_proof") }).unwrap();
    // D: Two (1)
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    // In current implementation, if player has BJ, it waits for dealer hole card reveal
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [3], next_status: DealerTurn }");
    
    // Reveal dealer hole card: Five (4) -> Dealer has 2+5=7. Player wins with BJ.
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 3, partial_decryption: Binary::from(&[4]), proof: Binary::from(b"valid_proof") }).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    // Dealer has 7, must hit!
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [4], next_status: DealerTurn }");

    // Reveal Dealer card 4: Ten (9) -> Dealer has 7+10=17. Stands.
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 4, partial_decryption: Binary::from(&[9]), proof: Binary::from(b"valid_proof") }).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    assert_eq!(game.status, "Settled { winner: \"Player (Blackjack)\" }");
    assert_eq!(game.hands[0].status, "Settled { winner: \"Player (Blackjack)\" }");
}

#[test]
fn test_surrender() {
    let mut deps = mock_dependencies();
    let inst_msg = InstantiateMsg {
        min_bet: Uint128::new(10),
        max_bet: Uint128::new(1000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 3,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "s".to_string(),
        reveal_vk_id: "r".to_string(),
    };
    instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("c"), &[]), inst_msg).unwrap();
    
    let player = Addr::unchecked("p");
    let info = message_info(&player, &[]);
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[0]), proof: Binary::from(b"valid_proof") }).unwrap();

    let res = execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Surrender {}).unwrap();
    assert_eq!(res.attributes.iter().find(|a| a.key == "refund_amount").unwrap().value, "50");

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    assert_eq!(game.status, "Settled { winner: \"Surrendered\" }");
    assert_eq!(game.hands[0].status, "Surrendered");
}

#[test]
fn proper_initialization() {
    let mut deps = mock_dependencies();
    let creator = "cosmwasm1zg63vla7v7svzpxatp6y0v5fuv8vml5u7e66ax";

    let msg = InstantiateMsg {
        min_bet: Uint128::new(100),
        max_bet: Uint128::new(10000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 3,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "shuffle_key".to_string(),
        reveal_vk_id: "reveal_key".to_string(),
    };
    let info = message_info(&Addr::unchecked(creator), &coins(1000, "earth"));

    let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    let res = query(deps.as_ref(), mock_env(), QueryMsg::GetConfig {}).unwrap();
    let value: Config = from_json(&res).unwrap();
    assert_eq!(Uint128::new(100), value.min_bet);
    assert_eq!("shuffle_key", value.shuffle_vk_id);
}

#[test]
fn test_split() {
    let mut deps = mock_dependencies();
    let inst_msg = InstantiateMsg {
        min_bet: Uint128::new(10),
        max_bet: Uint128::new(1000),
        bj_payout_permille: 1500,
        insurance_payout_permille: 2000,
        standard_payout_permille: 1000,
        dealer_hits_soft_17: true,
        dealer_peeks: true,
        double_down_restriction: crate::state::DoubleDownRestriction::Any,
        max_splits: 1,
        can_split_aces: true,
        can_hit_split_aces: false,
        surrender_allowed: true,
        shuffle_vk_id: "s".to_string(),
        reveal_vk_id: "r".to_string(),
    };
    instantiate(deps.as_mut(), mock_env(), message_info(&Addr::unchecked("c"), &[]), inst_msg).unwrap();
    
    let player = Addr::unchecked("p");
    let info = message_info(&player, &[]);
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::JoinGame { bet: Uint128::new(100), public_key: Binary::from(b"pk") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitShuffle { shuffled_deck: vec![Binary::from(b"c"); 52], proof: Binary::from(b"valid_proof") }).unwrap();

    // P: 8 (index 7), 8 (index 7) -> Pair
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 0, partial_decryption: Binary::from(&[7]), proof: Binary::from(b"valid_proof") }).unwrap();
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 1, partial_decryption: Binary::from(&[7]), proof: Binary::from(b"valid_proof") }).unwrap();
    // D: 2 (index 1)
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 2, partial_decryption: Binary::from(&[1]), proof: Binary::from(b"valid_proof") }).unwrap();

    // Split!
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Split {}).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    assert_eq!(game.hands.len(), 2);
    assert_eq!(game.hands[0].cards.len(), 1);
    assert_eq!(game.hands[1].cards.len(), 1);
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [4, 5], next_status: PlayerTurn }");

    // Reveal new cards for both hands
    // Hand 0 gets 10 (index 9) -> 8 + 10 = 18
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 4, partial_decryption: Binary::from(&[9]), proof: Binary::from(b"valid_proof") }).unwrap();
    // Hand 1 gets 3 (index 2) -> 8 + 3 = 11
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 5, partial_decryption: Binary::from(&[2]), proof: Binary::from(b"valid_proof") }).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    assert_eq!(game.hands[0].cards, vec![7, 9]);
    assert_eq!(game.hands[1].cards, vec![7, 2]);

    // Hand 0 is active (index 0). Stand on 18.
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Stand {}).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    // Current hand should now be 1
    assert_eq!(game.status, "PlayerTurn");
    assert!(game.hands[0].status.contains("Stood"));

    // Hand 1: Hit on 11
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::Hit {}).unwrap();
    // Hand 1 gets another 10 (index 9) -> 11 + 10 = 21
    execute(deps.as_mut(), mock_env(), info.clone(), ExecuteMsg::SubmitReveal { card_index: 6, partial_decryption: Binary::from(&[9]), proof: Binary::from(b"valid_proof") }).unwrap();

    let game: GameResponse = from_json(query(deps.as_ref(), mock_env(), QueryMsg::GetGame { player: player.to_string() }).unwrap()).unwrap();
    // 21 should automatically stand and move to dealer
    assert_eq!(game.status, "WaitingForReveal { reveal_requests: [3], next_status: DealerTurn }");
}
