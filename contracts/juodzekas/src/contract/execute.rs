use cosmwasm_std::{Addr, Binary, DepsMut, Env, MessageInfo, Order, Response, StdError, Uint128};
use crate::error::ContractError;
use crate::game_logic::{config_to_rules, to_blackjack_state};
use crate::msg::ExecuteMsg;
use crate::state::{GameSession, GameStatus, Hand, HandStatus, TurnOwner, CONFIG, DEALER, DEALER_BALANCE, GAMES, GAME_COUNTER};
use crate::zk::xion_zk_verify;

/// Reject messages that send unexpected funds
fn no_funds(info: &MessageInfo) -> Result<(), ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::Std(StdError::msg("Unexpected funds sent")));
    }
    Ok(())
}

/// Reject messages that include coins in any denom other than the expected one.
/// Prevents tokens from getting permanently stuck in the contract.
fn only_denom(info: &MessageInfo, denom: &str) -> Result<(), ContractError> {
    if info.funds.iter().any(|c| c.denom != denom) {
        return Err(ContractError::Std(StdError::msg(format!(
            "Only {denom} accepted; other denoms would be permanently locked"
        ))));
    }
    Ok(())
}

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
            bet,
            public_key,
            shuffled_deck,
            proof,
            public_inputs,
        } => execute_join_game(deps, _env, info, bet, public_key, shuffled_deck, proof, public_inputs),
        ExecuteMsg::Hit { game_id } => execute_hit(deps, _env, info, game_id),
        ExecuteMsg::Stand { game_id } => execute_stand(deps, _env, info, game_id),
        ExecuteMsg::DoubleDown { game_id } => execute_double_down(deps, _env, info, game_id),
        ExecuteMsg::Split { game_id } => execute_split(deps, _env, info, game_id),
        ExecuteMsg::Surrender { game_id } => execute_surrender(deps, _env, info, game_id),
        ExecuteMsg::Insurance { game_id } => execute_insurance(deps, _env, info, game_id),
        ExecuteMsg::DeclineInsurance { game_id } => execute_decline_insurance(deps, _env, info, game_id),
        ExecuteMsg::SubmitReveal {
            game_id,
            card_index,
            partial_decryption,
            proof,
            public_inputs,
        } => execute_submit_reveal(deps, _env, info, game_id, card_index, partial_decryption, proof, public_inputs),
        ExecuteMsg::CancelGame { game_id } => execute_cancel_game(deps, info, game_id),
        ExecuteMsg::ClaimTimeout { game_id } => execute_claim_timeout(deps, _env, info, game_id),
        ExecuteMsg::SweepSettled { game_ids } => execute_sweep_settled(deps, _env, game_ids),
        ExecuteMsg::DepositBankroll {} => execute_deposit_bankroll(deps, info),
        ExecuteMsg::WithdrawBankroll { amount } => execute_withdraw_bankroll(deps, info, amount),
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
    only_denom(&info, &config.denom)?;
    let dealer = DEALER.load(deps.storage)?;

    // Only the contract's dealer can create games
    if info.sender != dealer {
        return Err(ContractError::Std(StdError::msg("Only the dealer can create games")));
    }

    if shuffled_deck.len() != 52 {
        return Err(ContractError::Std(StdError::msg("Deck must contain exactly 52 cards")));
    }

    // Verify dealer's shuffle proof
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;
    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid dealer shuffle proof")));
    }

    // Dealer must have sufficient bankroll (10x max bet)
    let required_bankroll = config.max_bet.checked_mul(Uint128::new(10))
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;

    // Load existing dealer balance and add any sent funds
    let mut dealer_balance = DEALER_BALANCE.load(deps.storage)?;

    let deposited: Uint128 = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| Uint128::try_from(c.amount).unwrap_or(Uint128::MAX))
        .unwrap_or(Uint128::zero());

    dealer_balance = dealer_balance.checked_add(deposited)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;

    if dealer_balance < required_bankroll {
        return Err(ContractError::Std(StdError::msg(format!(
            "Insufficient bankroll. Required: {required_bankroll}, Available: {dealer_balance}"
        ))));
    }

    // Deduct bankroll from dealer balance
    dealer_balance = dealer_balance.checked_sub(required_bankroll)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &dealer_balance)?;

    // Generate new game ID
    let game_id = GAME_COUNTER.load(deps.storage)?;
    let next_id = game_id.checked_add(1)
        .ok_or_else(|| ContractError::Std(StdError::msg("Game counter overflow")))?;
    GAME_COUNTER.save(deps.storage, &next_id)?;

    // Create game session waiting for player
    let game = GameSession {
        player: Addr::unchecked("pending"), // Placeholder until player joins
        dealer: info.sender.clone(),
        bet: Uint128::zero(),
        bankroll: required_bankroll,
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
        dealer_peeked: false,
        insurance_bet: None,
    };

    // Store game by ID
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "create_game")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("dealer", info.sender)
        .add_attribute("bankroll", required_bankroll))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_join_game(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bet: Uint128,
    public_key: Binary,
    shuffled_deck: Vec<Binary>,
    proof: Binary,
    public_inputs: Vec<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    only_denom(&info, &config.denom)?;

    // Validate bet
    if bet < config.min_bet {
        return Err(ContractError::Std(StdError::msg("Bet too low")));
    }
    if bet > config.max_bet {
        return Err(ContractError::Std(StdError::msg("Bet too high")));
    }

    if shuffled_deck.len() != 52 {
        return Err(ContractError::Std(StdError::msg("Deck must contain exactly 52 cards")));
    }

    // Auto-find first WaitingForPlayerJoin game
    let (game_id, mut game) = GAMES
        .range(deps.storage, None, None, Order::Ascending)
        .find_map(|item| {
            let (id, g) = item.ok()?;
            if g.status == GameStatus::WaitingForPlayerJoin {
                Some((id, g))
            } else {
                None
            }
        })
        .ok_or_else(|| ContractError::Std(StdError::msg("No game available for joining")))?;

    // Dealer cannot join own game (self-play)
    if info.sender == game.dealer {
        return Err(ContractError::Std(StdError::msg("Dealer cannot join own game")));
    }

    // Verify player's re-shuffle proof
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;
    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid player shuffle proof")));
    }

    // Player must deposit exact bet in correct denom
    let deposited = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| c.amount)
        .unwrap_or(cosmwasm_std::Uint256::zero());

    let bet_u256 = cosmwasm_std::Uint256::from(bet);
    if deposited != bet_u256 {
        return Err(ContractError::Std(StdError::msg(format!(
            "Must send exact bet amount. Required: {bet_u256}, Got: {deposited}"
        ))));
    }

    // Update game with player info
    game.player = info.sender.clone();
    game.bet = bet;
    game.player_pubkey = public_key;
    game.deck = shuffled_deck; // Player's re-shuffle becomes final deck
    game.player_shuffled_deck = None; // Dealer's initial shuffle no longer needed on-chain
    game.hands = vec![Hand {
        cards: vec![],
        bet,
        status: HandStatus::Active,
    }];
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![0, 1, 2], // First 3 cards: player card 1, player card 2, dealer upcard
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.current_turn = TurnOwner::Player;
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
    no_funds(&info)?;
    let config = CONFIG.load(deps.storage)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    // Verify sender is player
    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }

    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    let hand_idx = game.current_hand_index as usize;
    if hand_idx >= game.hands.len() {
        return Err(ContractError::Std(StdError::msg("Invalid hand index")));
    }

    game.current_turn = crate::state::TurnOwner::Player;
    game.last_action_timestamp = env.block.time.seconds();

    let hand = &game.hands[hand_idx];
    if hand.status != HandStatus::Active {
        return Err(ContractError::Std(StdError::msg("Hand is not active")));
    }

    // Block hitting on split aces when config disallows it
    if !config.can_hit_split_aces && game.hands.len() > 1 {
        if let Some(&first_card) = hand.cards.first() {
            let rank = (first_card % 13) + 1;
            if rank == 1 && hand.cards.len() == 2 {
                return Err(ContractError::Std(StdError::msg("Cannot hit on split aces")));
            }
        }
    }

    if game.last_card_index >= 52 {
        return Err(ContractError::Std(StdError::msg("Deck exhausted")));
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

pub fn execute_stand(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    no_funds(&info)?;
    let config = CONFIG.load(deps.storage)?;
    let mut game = GAMES.load(deps.storage, game_id)?;
    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    let hand_idx = game.current_hand_index as usize;
    if hand_idx >= game.hands.len() {
        return Err(ContractError::Std(StdError::msg("Invalid hand index")));
    }
    let hand = &mut game.hands[hand_idx];
    if hand.status != HandStatus::Active {
        return Err(ContractError::Std(StdError::msg("Hand is not active")));
    }
    hand.status = HandStatus::Stood;

    game.status = super::reveal::advance_or_dealer_turn(&mut game, &config)?;
    game.last_action_timestamp = env.block.time.seconds();

    let mut response = Response::new()
        .add_attribute("action", "stand")
        .add_attribute("player", info.sender.clone())
        .add_attribute("hand_index", hand_idx.to_string());

    // If dealer peeked and all hands done, process_dealer_turn may have settled
    if matches!(game.status, GameStatus::Settled { .. }) {
        response = super::reveal::execute_payouts(deps.storage, &game, &config, response)?;
    }

    GAMES.save(deps.storage, game_id, &game)?;

    Ok(response)
}

pub fn execute_double_down(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    only_denom(&info, &config.denom)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    // Use blackjack package to validate all double rules (including restriction)
    let rules = config_to_rules(&config);
    let bj_state = to_blackjack_state(&game, rules);
    if !bj_state.can_double_current_hand() {
        return Err(ContractError::Std(StdError::msg("Double down not allowed")));
    }

    // Player must send exact additional bet equal to original hand bet
    let hand_idx = game.current_hand_index as usize;
    if hand_idx >= game.hands.len() {
        return Err(ContractError::Std(StdError::msg("Invalid hand index")));
    }
    let hand = &mut game.hands[hand_idx];
    let additional_bet = hand.bet;
    let deposited: Uint128 = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| Uint128::try_from(c.amount).unwrap_or(Uint128::MAX))
        .unwrap_or(Uint128::zero());
    if deposited != additional_bet {
        return Err(ContractError::Std(StdError::msg(format!(
            "Must send exact additional bet for double down. Required: {additional_bet}, Got: {deposited}"
        ))));
    }

    // Double the bet
    hand.bet = hand.bet.checked_add(additional_bet).map_err(|e| StdError::msg(e.to_string()))?;
    hand.status = HandStatus::Doubled;

    // Request exactly one card and transition to dealer turn
    if game.last_card_index >= 52 {
        return Err(ContractError::Std(StdError::msg("Deck exhausted")));
    }
    let card_to_reveal = game.last_card_index;
    game.last_card_index += 1;
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![card_to_reveal],
        next_status: Box::new(GameStatus::PlayerTurn), // Card goes to player; determine_next_status handles Doubled → DealerTurn
    };

    let doubled_bet = game.hands[hand_idx].bet;
    game.last_action_timestamp = env.block.time.seconds();
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "double_down")
        .add_attribute("player", info.sender)
        .add_attribute("new_bet", doubled_bet)
        .add_attribute("requested_card", card_to_reveal.to_string()))
}

pub fn execute_surrender(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    no_funds(&info)?;
    let mut game = GAMES.load(deps.storage, game_id)?;
    let config = CONFIG.load(deps.storage)?;

    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg("Not player turn")));
    }

    // Block surrender after split — accounting only handles single-hand games
    if game.hands.len() > 1 {
        return Err(ContractError::Std(StdError::msg("Surrender not allowed after split")));
    }

    // Use blackjack package to validate if surrender is allowed
    let rules = config_to_rules(&config);
    let bj_state = to_blackjack_state(&game, rules);
    if !bj_state.can_surrender_current_hand() {
        return Err(ContractError::Std(StdError::msg("Surrender not allowed")));
    }

    let hand = &mut game.hands[game.current_hand_index as usize];

    // Settlement: return half the bet to player
    let refund_amount = hand.bet.checked_div(Uint128::new(2)).map_err(|e| StdError::msg(e.to_string()))?;

    // Credit dealer: bankroll + player's bet + lost insurance - player's refund
    let insurance_bet = game.insurance_bet.unwrap_or(Uint128::zero());
    let dealer_credit = game.bankroll
        .checked_add(hand.bet).map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?
        .checked_add(insurance_bet).map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?
        .checked_sub(refund_amount).map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    let mut dealer_balance = DEALER_BALANCE.load(deps.storage)?;
    dealer_balance = dealer_balance.checked_add(dealer_credit)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &dealer_balance)?;

    hand.status = HandStatus::Surrendered;
    game.current_turn = crate::state::TurnOwner::None;
    game.status = GameStatus::Settled {
        winner: "Surrendered".to_string(),
    };

    game.last_action_timestamp = env.block.time.seconds();
    GAMES.save(deps.storage, game_id, &game)?;

    let mut response = Response::new()
        .add_attribute("action", "surrender")
        .add_attribute("player", info.sender)
        .add_attribute("refund_amount", refund_amount);

    if refund_amount > Uint128::zero() {
        response = response.add_message(cosmwasm_std::BankMsg::Send {
            to_address: game.player.to_string(),
            amount: vec![cosmwasm_std::Coin {
                denom: config.denom,
                amount: refund_amount.into(),
            }],
        });
    }

    Ok(response)
}

pub fn execute_split(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    only_denom(&info, &config.denom)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
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
    let original_bet = game.hands[hand_index].bet;

    // Player must send exact additional bet equal to original hand bet for the new split hand
    let deposited: Uint128 = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| Uint128::try_from(c.amount).unwrap_or(Uint128::MAX))
        .unwrap_or(Uint128::zero());
    if deposited != original_bet {
        return Err(ContractError::Std(StdError::msg(format!(
            "Must send exact additional bet for split. Required: {original_bet}, Got: {deposited}"
        ))));
    }

    let (card0, card1) = {
        let hand = &game.hands[hand_index];
        (hand.cards[0], hand.cards[1])
    };

    // Split the hand
    game.hands[hand_index].cards = vec![card0];
    game.hands.push(Hand {
        cards: vec![card1],
        bet: original_bet,
        status: HandStatus::Active,
    });

    // Request two cards, one for each hand
    if game.last_card_index + 1 >= 52 {
        return Err(ContractError::Std(StdError::msg("Deck exhausted")));
    }
    let card_to_reveal_1 = game.last_card_index;
    let card_to_reveal_2 = game.last_card_index + 1;
    game.last_card_index += 2;

    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![card_to_reveal_1, card_to_reveal_2],
        next_status: Box::new(GameStatus::PlayerTurn),
    };

    game.last_action_timestamp = env.block.time.seconds();
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "split")
        .add_attribute("player", info.sender)
        .add_attribute("requested_cards", format!("{card_to_reveal_1}, {card_to_reveal_2}")))
}

pub fn execute_insurance(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    only_denom(&info, &config.denom)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
    if game.status != GameStatus::OfferingInsurance {
        return Err(ContractError::Std(StdError::msg("Insurance not being offered")));
    }

    let insurance_amount = game.bet.checked_div(Uint128::new(2))
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;

    let deposited = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| c.amount)
        .unwrap_or(cosmwasm_std::Uint256::zero());

    let required = cosmwasm_std::Uint256::from(insurance_amount);
    if deposited != required {
        return Err(ContractError::Std(StdError::msg(format!(
            "Must send exact insurance amount. Required: {required}, Got: {deposited}"
        ))));
    }

    game.insurance_bet = Some(insurance_amount);
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![3],
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.last_action_timestamp = env.block.time.seconds();
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "insurance")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("insurance_amount", insurance_amount))
}

pub fn execute_decline_insurance(deps: DepsMut, env: Env, info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    no_funds(&info)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    if game.player != info.sender {
        return Err(ContractError::Std(StdError::msg("Not authorized")));
    }
    if game.status != GameStatus::OfferingInsurance {
        return Err(ContractError::Std(StdError::msg("Insurance not being offered")));
    }

    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![3],
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.last_action_timestamp = env.block.time.seconds();
    GAMES.save(deps.storage, game_id, &game)?;

    Ok(Response::new()
        .add_attribute("action", "decline_insurance")
        .add_attribute("game_id", game_id.to_string()))
}

// Import from reveal module
use super::reveal::execute_submit_reveal;

pub fn execute_cancel_game(
    deps: DepsMut,
    info: MessageInfo,
    game_id: u64,
) -> Result<Response, ContractError> {
    no_funds(&info)?;
    let dealer = DEALER.load(deps.storage)?;
    if info.sender != dealer {
        return Err(ContractError::Std(StdError::msg("Only the dealer can cancel games")));
    }

    let game = GAMES.load(deps.storage, game_id)?;
    if game.status != GameStatus::WaitingForPlayerJoin {
        return Err(ContractError::Std(StdError::msg("Can only cancel games waiting for a player")));
    }

    // Return bankroll to dealer balance
    let mut dealer_balance = DEALER_BALANCE.load(deps.storage)?;
    dealer_balance = dealer_balance.checked_add(game.bankroll)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &dealer_balance)?;

    GAMES.remove(deps.storage, game_id);

    Ok(Response::new()
        .add_attribute("action", "cancel_game")
        .add_attribute("game_id", game_id.to_string())
        .add_attribute("returned_bankroll", game.bankroll))
}

pub fn execute_claim_timeout(deps: DepsMut, env: Env, _info: MessageInfo, game_id: u64) -> Result<Response, ContractError> {
    no_funds(&_info)?;
    let config = CONFIG.load(deps.storage)?;
    let mut game = GAMES.load(deps.storage, game_id)?;

    // Reject if game is already settled
    if matches!(game.status, GameStatus::Settled { .. }) {
        return Err(ContractError::Std(StdError::msg("Game is already settled")));
    }

    let current_time = env.block.time.seconds();
    let time_elapsed = current_time.saturating_sub(game.last_action_timestamp);

    let timeout = config.timeout_seconds;
    if time_elapsed < timeout {
        return Err(ContractError::Std(StdError::msg(format!(
            "Timeout not reached. Elapsed: {time_elapsed}s, Required: {timeout}s"
        ))));
    }

    let total_bets: Uint128 = game.hands.iter().map(|h| h.bet).sum();
    let insurance_bet = game.insurance_bet.unwrap_or(Uint128::zero());

    // Determine who is blocking progress.
    // During WaitingForReveal, both parties must submit — check pending_reveals to
    // see who submitted more partials; the lagging party is the blocker.
    let blocker = if matches!(&game.status, GameStatus::WaitingForReveal { .. }) {
        let player_count = game.pending_reveals.iter()
            .filter(|pr| pr.player_partial.is_some()).count();
        let dealer_count = game.pending_reveals.iter()
            .filter(|pr| pr.dealer_partial.is_some()).count();
        if player_count > dealer_count {
            crate::state::TurnOwner::Dealer // dealer is lagging
        } else if dealer_count > player_count {
            crate::state::TurnOwner::Player // player is lagging
        } else {
            game.current_turn.clone() // equal, fall back
        }
    } else {
        game.current_turn.clone()
    };

    let (player_payout, dealer_credit, winner) = match blocker {
        crate::state::TurnOwner::Player => {
            // Player failed to act, dealer wins: gets bankroll + all player bets + insurance
            let credit = game.bankroll.checked_add(total_bets)
                .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?
                .checked_add(insurance_bet)
                .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            (Uint128::zero(), credit, "Dealer")
        }
        crate::state::TurnOwner::Dealer => {
            // Dealer failed to act, player wins 2x bets + insurance back
            let payout = total_bets.checked_mul(Uint128::new(2))
                .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?
                .checked_add(insurance_bet)
                .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            // Dealer gets back bankroll minus what player takes from it
            let credit = game.bankroll.saturating_sub(total_bets);
            (payout, credit, "Player")
        }
        crate::state::TurnOwner::None => {
            return Err(ContractError::Std(StdError::msg(
                "No active turn to timeout",
            )));
        }
    };

    // Credit dealer balance
    let mut dealer_balance = DEALER_BALANCE.load(deps.storage)?;
    dealer_balance = dealer_balance.checked_add(dealer_credit)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &dealer_balance)?;

    // Mark game as settled instead of removing
    game.status = GameStatus::Settled { winner: winner.to_string() };
    game.current_turn = crate::state::TurnOwner::None;
    game.last_action_timestamp = current_time;
    GAMES.save(deps.storage, game_id, &game)?;

    // Build response with payout if player wins
    let mut response = Response::new()
        .add_attribute("action", "claim_timeout")
        .add_attribute("winner", winner)
        .add_attribute("elapsed_seconds", time_elapsed.to_string());

    if player_payout > Uint128::zero() {
        response = response.add_message(cosmwasm_std::BankMsg::Send {
            to_address: game.player.to_string(),
            amount: vec![cosmwasm_std::Coin {
                denom: config.denom,
                amount: player_payout.into(),
            }],
        });
    }

    Ok(response)
}

pub fn execute_sweep_settled(deps: DepsMut, env: Env, game_ids: Vec<u64>) -> Result<Response, ContractError> {
    if game_ids.len() > 50 {
        return Err(ContractError::Std(StdError::msg("Cannot sweep more than 50 games at once")));
    }
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
        if game.last_action_timestamp.saturating_add(config.timeout_seconds) > now {
            continue;
        }
        GAMES.remove(deps.storage, *game_id);
        removed += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "sweep_settled")
        .add_attribute("removed", removed.to_string()))
}

pub fn execute_deposit_bankroll(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    only_denom(&info, &config.denom)?;
    let dealer = DEALER.load(deps.storage)?;

    if info.sender != dealer {
        return Err(ContractError::Std(StdError::msg("Only the dealer can deposit bankroll")));
    }

    let deposited: Uint128 = info.funds.iter()
        .find(|c| c.denom == config.denom)
        .map(|c| Uint128::try_from(c.amount).unwrap_or(Uint128::MAX))
        .unwrap_or(Uint128::zero());

    if deposited.is_zero() {
        return Err(ContractError::Std(StdError::msg("No funds sent")));
    }

    let mut balance = DEALER_BALANCE.load(deps.storage)?;
    balance = balance.checked_add(deposited)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &balance)?;

    Ok(Response::new()
        .add_attribute("action", "deposit_bankroll")
        .add_attribute("deposited", deposited)
        .add_attribute("new_balance", balance))
}

pub fn execute_withdraw_bankroll(
    deps: DepsMut,
    info: MessageInfo,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let dealer = DEALER.load(deps.storage)?;

    if info.sender != dealer {
        return Err(ContractError::Std(StdError::msg("Only the dealer can withdraw bankroll")));
    }

    // Block withdrawal if any unsettled game exists (including WaitingForPlayerJoin).
    // Dealer should cancel waiting games first, then withdraw.
    let has_unsettled = GAMES
        .range(deps.storage, None, None, Order::Ascending)
        .any(|item| {
            if let Ok((_, g)) = item {
                !matches!(g.status, GameStatus::Settled { .. })
            } else {
                false
            }
        });
    if has_unsettled {
        return Err(ContractError::Std(StdError::msg(
            "Cannot withdraw while unsettled games exist. Cancel waiting games first."
        )));
    }

    let balance = DEALER_BALANCE.load(deps.storage)?;

    let withdraw_amount = amount.unwrap_or(balance);

    if withdraw_amount.is_zero() {
        return Err(ContractError::Std(StdError::msg("Nothing to withdraw")));
    }
    if withdraw_amount > balance {
        return Err(ContractError::Std(StdError::msg(format!(
            "Insufficient balance. Available: {balance}, Requested: {withdraw_amount}"
        ))));
    }

    let new_balance = balance.checked_sub(withdraw_amount)
        .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
    DEALER_BALANCE.save(deps.storage, &new_balance)?;

    Ok(Response::new()
        .add_message(cosmwasm_std::BankMsg::Send {
            to_address: dealer.to_string(),
            amount: vec![cosmwasm_std::Coin {
                denom: config.denom,
                amount: withdraw_amount.into(),
            }],
        })
        .add_attribute("action", "withdraw_bankroll")
        .add_attribute("amount", withdraw_amount)
        .add_attribute("remaining", new_balance))
}
