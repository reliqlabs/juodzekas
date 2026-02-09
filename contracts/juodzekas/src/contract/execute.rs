use cosmwasm_std::{Addr, Binary, DepsMut, Env, MessageInfo, Response, StdError, Uint128};
use crate::error::ContractError;
use crate::game_logic::{config_to_rules, to_blackjack_state};
use crate::msg::ExecuteMsg;
use crate::state::{GameSession, GameStatus, Hand, HandStatus, TurnOwner, CONFIG, GAMES, GAME_COUNTER};
use crate::zk::xion_zk_verify;

pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreateGame {
            public_key,
            shuffled_deck,
            proof,
            public_inputs,
        } => execute_create_game(deps, _env, info, public_key, shuffled_deck, proof, public_inputs),
        ExecuteMsg::JoinGame {
            game_id,
            bet,
            public_key,
            shuffled_deck,
            proof,
            public_inputs,
        } => execute_join_game(deps, _env, info, game_id, bet, public_key, shuffled_deck, proof, public_inputs),
        ExecuteMsg::Hit { game_id } => execute_hit(deps, _env, info, game_id),
        ExecuteMsg::Stand { game_id } => execute_stand(deps, info, game_id),
        ExecuteMsg::DoubleDown { game_id } => execute_double_down(deps, info, game_id),
        ExecuteMsg::Split { game_id } => execute_split(deps, info, game_id),
        ExecuteMsg::Surrender { game_id } => execute_surrender(deps, info, game_id),
        ExecuteMsg::SubmitReveal {
            game_id,
            card_index,
            partial_decryption,
            proof,
            public_inputs,
        } => execute_submit_reveal(deps, _env, info, game_id, card_index, partial_decryption, proof, public_inputs),
        ExecuteMsg::ClaimTimeout { game_id } => execute_claim_timeout(deps, _env, info, game_id),
        ExecuteMsg::SweepSettled { game_ids } => execute_sweep_settled(deps, _env, game_ids),
    }
}

pub fn execute_create_game(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    public_key: Binary,
    shuffled_deck: Vec<Binary>,
    proof: Binary,
    public_inputs: Vec<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Verify dealer's shuffle proof
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;
    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid dealer shuffle proof")));
    }

    // Dealer must deposit bankroll (e.g., 10x max bet to cover player wins)
    let required_bankroll = config.max_bet.checked_mul(Uint128::new(10))
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;

    let deposited = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| c.amount)
        .unwrap_or(cosmwasm_std::Uint256::zero());

    let required_bankroll_u256 = cosmwasm_std::Uint256::from(required_bankroll);
    if deposited < required_bankroll_u256 {
        return Err(ContractError::Std(StdError::msg(format!(
            "Insufficient bankroll. Required: {required_bankroll_u256}, Got: {deposited}"
        ))));
    }

    // Generate new game ID
    let game_id = GAME_COUNTER.load(deps.storage)?;
    GAME_COUNTER.save(deps.storage, &(game_id + 1))?;

    // Create game session waiting for player
    let game = GameSession {
        player: Addr::unchecked("pending"), // Placeholder until player joins
        dealer: info.sender.clone(),
        bet: Uint128::zero(),
        player_pubkey: Binary::default(),
        dealer_pubkey: public_key,
        deck: vec![],
        player_shuffled_deck: Some(shuffled_deck), // Dealer's initial shuffle
        hands: vec![],
        current_hand_index: 0,
        dealer_hand: vec![],
        status: GameStatus::WaitingForPlayerJoin,
        current_turn: TurnOwner::None,
        last_action_timestamp: env.block.time.seconds(),
        last_card_index: 0,
        pending_reveals: vec![],
    };

    // Store game by ID
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "create_game")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("dealer", info.sender)
        .add_attribute("bankroll", deposited))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_join_game(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    game_id: u64,
    bet: Uint128,
    public_key: Binary,
    shuffled_deck: Vec<Binary>,
    proof: Binary,
    public_inputs: Vec<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Validate bet
    if bet < config.min_bet {
        return Err(ContractError::Std(StdError::msg("Bet too low")));
    }
    if bet > config.max_bet {
        return Err(ContractError::Std(StdError::msg("Bet too high")));
    }

    // Load game by ID
    let mut game = GAMES.load(deps.storage, game_id)?;

    // Verify game is waiting for player
    if game.status != GameStatus::WaitingForPlayerJoin {
        return Err(ContractError::Std(StdError::msg("Game not available for joining")));
    }

    // Verify player's re-shuffle proof
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;
    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid player shuffle proof")));
    }

    // Player must deposit bet in correct denom
    let deposited = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| c.amount)
        .unwrap_or(cosmwasm_std::Uint256::zero());

    let bet_u256 = cosmwasm_std::Uint256::from(bet);
    if deposited < bet_u256 {
        return Err(ContractError::Std(StdError::msg(format!(
            "Insufficient bet. Required: {bet_u256}, Got: {deposited}"
        ))));
    }

    // Update game with player info
    game.player = info.sender.clone();
    game.bet = bet;
    game.player_pubkey = public_key;
    game.deck = shuffled_deck; // Player's re-shuffle becomes final deck
    game.hands = vec![Hand {
        cards: vec![],
        bet,
        status: HandStatus::Active,
    }];
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![0, 1, 2], // First 3 cards: player card 1, player card 2, dealer upcard
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.last_card_index = 4; // Cards 0-3 are dealt (player gets 0,1 and dealer gets 2,3)
    game.last_action_timestamp = env.block.time.seconds();

    // Save updated game
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "join_game")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("player", info.sender)
        .add_attribute("dealer", game.dealer)
        .add_attribute("bet", bet))
}


pub fn execute_hit(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, game_id)?;

    // Verify sender is player
    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }

    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    game.current_turn = crate::state::TurnOwner::Player;
    game.last_action_timestamp = env.block.time.seconds();

    let hand = &game.hands[game.current_hand_index as usize];
    if hand.status != HandStatus::Active {
        return Err(ContractError::Std(StdError::msg("Hand is not active")));
    }

    let card_to_reveal = game.last_card_index;
    game.last_card_index += 1;
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![card_to_reveal],
        next_status: Box::new(GameStatus::PlayerTurn),
    };

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "hit")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("player", info.sender)
        .add_attribute("requested_card", card_to_reveal.to_string())
        .add_attribute("hand_index", game.current_hand_index.to_string()))
}

pub fn execute_stand(deps: DepsMut, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, game_id)?;
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    let hand = &mut game.hands[game.current_hand_index as usize];
    hand.status = HandStatus::Stood;

    // Check if there are more hands to play
    if (game.current_hand_index as usize) + 1 < game.hands.len() {
        game.current_hand_index += 1;
        // The next hand might already be finished (e.g. if it was Blackjack)
        // But for now we just move to it.
    } else {
        // All hands finished, move to dealer turn
        game.status = GameStatus::WaitingForReveal {
            reveal_requests: vec![3],
            next_status: Box::new(GameStatus::DealerTurn),
        };
    }

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "stand")
        .add_attribute("player", info.sender)
        .add_attribute("hand_index", game.current_hand_index.to_string()))
}

pub fn execute_double_down(deps: DepsMut, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, game_id)?;
    let config = CONFIG.load(deps.storage)?;

    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    // Use blackjack package to validate all double rules (including restriction)
    let rules = config_to_rules(&config);
    let bj_state = to_blackjack_state(&game, rules);
    if !bj_state.can_double_current_hand() {
        return Err(ContractError::Std(StdError::msg("Double down not allowed")));
    }

    // Double the bet
    let hand = &mut game.hands[game.current_hand_index as usize];
    hand.bet = hand.bet.checked_mul(Uint128::new(2)).map_err(|e| StdError::msg(e.to_string()))?;
    hand.status = HandStatus::Doubled;

    // Request exactly one card and transition to dealer turn
    let card_to_reveal = game.last_card_index;
    game.last_card_index += 1;
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![card_to_reveal],
        next_status: Box::new(GameStatus::DealerTurn), // Force stand after one card
    };

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "double_down")
        .add_attribute("player", info.sender)
        .add_attribute("new_bet", game.bet)
        .add_attribute("requested_card", card_to_reveal.to_string()))
}

pub fn execute_surrender(deps: DepsMut, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, game_id)?;
    let config = CONFIG.load(deps.storage)?;

    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    // Use blackjack package to validate if surrender is allowed
    let rules = config_to_rules(&config);
    let bj_state = to_blackjack_state(&game, rules);
    if !bj_state.can_surrender_current_hand() {
        return Err(ContractError::Std(StdError::msg("Surrender not allowed")));
    }

    let hand = &mut game.hands[game.current_hand_index as usize];

    // Settlement: return half the bet
    let refund_amount = hand.bet.checked_div(Uint128::new(2)).map_err(|e| StdError::msg(e.to_string()))?;
    hand.status = HandStatus::Surrendered;
    game.status = GameStatus::Settled {
        winner: "Surrendered".to_string(),
    };

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "surrender")
        .add_attribute("player", info.sender)
        .add_attribute("refund_amount", refund_amount))
}

pub fn execute_split(deps: DepsMut, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, game_id)?;
    let config = CONFIG.load(deps.storage)?;

    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    // Use blackjack package to validate if split is allowed
    let rules = config_to_rules(&config);
    let bj_state = to_blackjack_state(&game, rules);
    if !bj_state.can_split_current_hand() {
        return Err(ContractError::Std(StdError::msg("Split not allowed")));
    }

    let hand_index = game.current_hand_index as usize;
    let (card0, card1) = {
        let hand = &game.hands[hand_index];
        (hand.cards[0], hand.cards[1])
    };

    let original_bet = game.hands[hand_index].bet;

    // Split the hand
    game.hands[hand_index].cards = vec![card0];
    game.hands.push(Hand {
        cards: vec![card1],
        bet: original_bet,
        status: HandStatus::Active,
    });

    // Request two cards, one for each hand
    let card_to_reveal_1 = game.last_card_index;
    let card_to_reveal_2 = game.last_card_index + 1;
    game.last_card_index += 2;

    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![card_to_reveal_1, card_to_reveal_2],
        next_status: Box::new(GameStatus::PlayerTurn),
    };

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "split")
        .add_attribute("player", info.sender)
        .add_attribute("requested_cards", format!("{card_to_reveal_1}, {card_to_reveal_2}")))
}

// Import from reveal module
use super::reveal::execute_submit_reveal;

pub fn execute_claim_timeout(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    let current_time = env.block.time.seconds();
    let time_elapsed = current_time.saturating_sub(game.last_action_timestamp);

    let timeout = config.timeout_seconds;
    if time_elapsed < timeout {
        return Err(ContractError::Std(StdError::msg(format!(
            "Timeout not reached. Elapsed: {time_elapsed}s, Required: {timeout}s"
        ))));
    }

    // Determine who wins based on whose turn it was
    let (refund_amount, winner) = match game.current_turn {
        crate::state::TurnOwner::Player => {
            // Player failed to act, dealer wins
            (Uint128::zero(), "Dealer")
        }
        crate::state::TurnOwner::Dealer => {
            // Dealer failed to act, player wins all bets
            let total_bet: Uint128 = game.hands.iter().map(|h| h.bet).sum();
            let payout = total_bet.checked_mul(Uint128::new(2))
                .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            (payout, "Player")
        }
        crate::state::TurnOwner::None => {
            return Err(ContractError::Std(StdError::msg(
                "No active turn to timeout",
            )));
        }
    };

    // Mark game as settled instead of removing
    game.status = GameStatus::Settled { winner: winner.to_string() };
    game.last_action_timestamp = current_time;
    GAMES.save(deps.storage, game_id, &game)?;

    // Build response with refund if player wins
    let mut response = Response::new()
        .add_attribute("action", "claim_timeout")
        .add_attribute("winner", winner)
        .add_attribute("elapsed_seconds", time_elapsed.to_string());

    if refund_amount > Uint128::zero() {
        response = response.add_message(cosmwasm_std::BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![cosmwasm_std::Coin {
                denom: config.denom,
                amount: refund_amount.into(),
            }],
        });
    }

    Ok(response)
}

pub fn execute_sweep_settled(deps: DepsMut, env: Env, game_ids: Vec<u64>) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let now = env.block.time.seconds();
    let mut removed = 0u64;

    for game_id in &game_ids {
        let game = match GAMES.may_load(deps.storage, *game_id)? {
            Some(g) => g,
            None => continue,
        };
        if !matches!(game.status, GameStatus::Settled { .. }) {
            continue;
        }
        if game.last_action_timestamp + config.timeout_seconds > now {
            continue;
        }
        GAMES.remove(deps.storage, *game_id);
        removed += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "sweep_settled")
        .add_attribute("removed", removed.to_string()))
}
