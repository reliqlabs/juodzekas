use cosmwasm_std::{Binary, DepsMut, Env, MessageInfo, Response, StdError, Uint128};
use crate::error::ContractError;
use crate::game_logic::{config_to_rules, to_blackjack_state};
use crate::msg::ExecuteMsg;
use crate::state::{GameSession, GameStatus, Hand, HandStatus, CONFIG, GAMES};
use crate::zk::xion_zk_verify;

pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::JoinGame { bet, public_key } => execute_join_game(deps, _env, info, bet, public_key),
        ExecuteMsg::SubmitShuffle {
            shuffled_deck,
            proof,
        } => execute_submit_shuffle(deps, info, shuffled_deck, proof),
        ExecuteMsg::Hit {} => execute_hit(deps, _env, info),
        ExecuteMsg::Stand {} => execute_stand(deps, info),
        ExecuteMsg::DoubleDown {} => execute_double_down(deps, info),
        ExecuteMsg::Split {} => execute_split(deps, info),
        ExecuteMsg::Surrender {} => execute_surrender(deps, info),
        ExecuteMsg::SubmitReveal {
            card_index,
            partial_decryption,
            proof,
        } => execute_submit_reveal(deps, info, card_index, partial_decryption, proof),
        ExecuteMsg::ClaimTimeout {} => execute_claim_timeout(deps, _env, info),
    }
}

pub fn execute_join_game(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bet: Uint128,
    public_key: Binary,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if bet < config.min_bet {
        return Err(ContractError::Std(StdError::msg("Bet too low")));
    }
    if bet > config.max_bet {
        return Err(ContractError::Std(StdError::msg("Bet too high")));
    }

    let game = GameSession {
        player: info.sender.clone(),
        bet,
        player_pubkey: public_key,
        dealer_pubkey: Binary::default(),
        deck: vec![],
        hands: vec![Hand {
            cards: vec![],
            bet,
            status: HandStatus::Active,
        }],
        current_hand_index: 0,
        dealer_hand: vec![],
        status: GameStatus::WaitingForShuffle,
        current_turn: crate::state::TurnOwner::None,
        last_action_timestamp: env.block.time.seconds(),
        last_card_index: 0,
    };

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "join_game")
        .add_attribute("player", info.sender))
}

pub fn execute_submit_shuffle(
    deps: DepsMut,
    info: MessageInfo,
    shuffled_deck: Vec<Binary>,
    proof: Binary,
) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
    let config = CONFIG.load(deps.storage)?;

    if game.status != GameStatus::WaitingForShuffle {
        return Err(ContractError::Std(StdError::msg("Invalid game status")));
    }

    let public_inputs = vec![];
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;

    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid shuffle proof")));
    }

    game.deck = shuffled_deck;
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![0, 1, 2],
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.last_card_index = 4;

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "submit_shuffle")
        .add_attribute("player", info.sender))
}

pub fn execute_hit(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
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

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "hit")
        .add_attribute("player", info.sender)
        .add_attribute("requested_card", card_to_reveal.to_string())
        .add_attribute("hand_index", game.current_hand_index.to_string()))
}

pub fn execute_stand(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
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

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "stand")
        .add_attribute("player", info.sender)
        .add_attribute("hand_index", game.current_hand_index.to_string()))
}

pub fn execute_double_down(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
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

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "double_down")
        .add_attribute("player", info.sender)
        .add_attribute("new_bet", game.bet)
        .add_attribute("requested_card", card_to_reveal.to_string()))
}

pub fn execute_surrender(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
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

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "surrender")
        .add_attribute("player", info.sender)
        .add_attribute("refund_amount", refund_amount))
}

pub fn execute_split(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
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

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "split")
        .add_attribute("player", info.sender)
        .add_attribute("requested_cards", format!("{card_to_reveal_1}, {card_to_reveal_2}")))
}

pub fn execute_submit_reveal(
    deps: DepsMut,
    info: MessageInfo,
    card_index: u32,
    partial_decryption: Binary,
    proof: Binary,
) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
    let config = CONFIG.load(deps.storage)?;

    let (mut reveal_requests, next_status) = match game.status {
        GameStatus::WaitingForReveal {
            reveal_requests,
            next_status,
        } => (reveal_requests, next_status),
        _ => {
            return Err(ContractError::Std(StdError::msg(format!(
                "No pending reveal. Current status: {:?}",
                game.status
            ))))
        }
    };

    if !reveal_requests.contains(&card_index) {
        return Err(ContractError::Std(StdError::msg(format!(
            "Invalid card reveal index: {card_index}. Expected one of: {reveal_requests:?}"
        ))));
    }

    let public_inputs = vec![
        card_index.to_string(),
        hex::encode(partial_decryption.as_slice()),
    ];
    let verified = xion_zk_verify(deps.as_ref(), &config.reveal_vk_id, proof, public_inputs)?;

    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid reveal proof")));
    }

    let card_value = partial_decryption.as_slice()[0] % 52;

    if card_index < 2 {
        game.hands[0].cards.push(card_value);
    } else if card_index == 2 || card_index == 3 {
        game.dealer_hand.push(card_value);
    } else {
        if let GameStatus::PlayerTurn = *next_status {
            // If it's PlayerTurn, we need to know which hand to add the card to.
            // During a Split, we might have multiple cards being revealed.
            // Simplified: add to the first hand that has fewer than 2 cards, 
            // OR if all have 2+, add to the current hand.
            // BUT during a Split reveal, card_to_reveal_1 was for hand current_hand_index
            // and card_to_reveal_2 was for the newly added hand.
            
            // For now, let's assume if it's PlayerTurn, it's for the current hand 
            // UNLESS it's a split reveal.
            // Actually, we can just use the game.current_hand_index for simple Hit/DoubleDown.
            // For Split, we need to be more careful.
            
            // Let's find the hand that "needs" a card.
            // In a simple Hit, it's the current hand.
            // In a Split, it's more complex.
            
            // A better way: track which card belongs to which hand in the reveal request.
            // But for now, let's just use current_hand_index.
            // If it's a split, card_index might be > 3.
            
            let mut hand_found = false;
            for hand in &mut game.hands {
                if hand.cards.len() < 2 {
                    hand.cards.push(card_value);
                    hand_found = true;
                    break;
                }
            }
            if !hand_found {
                game.hands[game.current_hand_index as usize].cards.push(card_value);
            }
        } else {
            game.dealer_hand.push(card_value);
        }
    }

    reveal_requests.retain(|&i| i != card_index);

    if reveal_requests.is_empty() {
        game.status = match *next_status {
            GameStatus::PlayerTurn => {
                let hand_index = game.current_hand_index as usize;
                let p_score = crate::contract::calculate_score(&game.hands[hand_index].cards);
                
                if p_score > 21 {
                    game.hands[hand_index].status = HandStatus::Busted;
                    // Move to next hand or DealerTurn
                    if hand_index + 1 < game.hands.len() {
                        game.current_hand_index += 1;
                        GameStatus::PlayerTurn
                    } else {
                        GameStatus::WaitingForReveal {
                            reveal_requests: vec![3],
                            next_status: Box::new(GameStatus::DealerTurn),
                        }
                    }
                } else if p_score == 21 && game.hands[hand_index].cards.len() == 2 {
                    // Blackjack!
                    game.hands[hand_index].status = HandStatus::Stood;
                    if hand_index + 1 < game.hands.len() {
                        game.current_hand_index += 1;
                        GameStatus::PlayerTurn
                    } else {
                        // If it's the last hand and it's Blackjack, we still need to reveal dealer's hole card
                        GameStatus::WaitingForReveal {
                            reveal_requests: vec![3],
                            next_status: Box::new(GameStatus::DealerTurn),
                        }
                    }
                } else if p_score == 21 {
                    // 21 but not Blackjack (e.g. after a split or hit)
                    game.hands[hand_index].status = HandStatus::Stood;
                    if hand_index + 1 < game.hands.len() {
                        game.current_hand_index += 1;
                        GameStatus::PlayerTurn
                    } else {
                        GameStatus::WaitingForReveal {
                            reveal_requests: vec![3],
                            next_status: Box::new(GameStatus::DealerTurn),
                        }
                    }
                } else if let HandStatus::Doubled = game.hands[hand_index].status {
                    // After Double Down, force stand
                    game.hands[hand_index].status = HandStatus::Stood;
                    if hand_index + 1 < game.hands.len() {
                        game.current_hand_index += 1;
                        GameStatus::PlayerTurn
                    } else {
                        GameStatus::WaitingForReveal {
                            reveal_requests: vec![3],
                            next_status: Box::new(GameStatus::DealerTurn),
                        }
                    }
                } else {
                    GameStatus::PlayerTurn
                }
            }
            GameStatus::DealerTurn => {
                let d_score = crate::contract::calculate_score(&game.dealer_hand);
                
                // Collect results for all hands
                let mut all_settled = true;
                let mut results = vec![];

                for hand in &mut game.hands {
                    if let HandStatus::Settled { .. } = hand.status {
                        results.push(format!("{:?}", hand.status));
                        continue;
                    }
                    if let HandStatus::Surrendered = hand.status {
                         hand.status = HandStatus::Settled { winner: "Surrendered".to_string() };
                         results.push("Surrendered".to_string());
                         continue;
                    }
                    if let HandStatus::Busted = hand.status {
                        hand.status = HandStatus::Settled { winner: "Dealer".to_string() };
                        results.push("Dealer".to_string());
                        continue;
                    }

                    let p_score = crate::contract::calculate_score(&hand.cards);
                    
                    if d_score > 21 {
                        hand.status = HandStatus::Settled { winner: "Player".to_string() };
                        results.push("Player".to_string());
                    } else if d_score >= 17 {
                        let mut should_hit = false;
                        if d_score == 17 && config.dealer_hits_soft_17 {
                            let mut score = 0;
                            let mut aces = 0;
                            for &card in &game.dealer_hand {
                                let val = (card % 13) + 1;
                                if val == 1 {
                                    aces += 1;
                                    score += 11;
                                } else if val > 10 {
                                    score += 10;
                                } else {
                                    score += val;
                                }
                            }
                            if aces > 0 && score == 17 {
                                should_hit = true;
                            }
                        }

                        if should_hit {
                            all_settled = false;
                            break;
                        } else {
                            if d_score > p_score {
                                hand.status = HandStatus::Settled { winner: "Dealer".to_string() };
                                results.push("Dealer".to_string());
                            } else if d_score < p_score {
                                if p_score == 21 && hand.cards.len() == 2 {
                                    hand.status = HandStatus::Settled { winner: "Player (Blackjack)".to_string() };
                                    results.push("Player (Blackjack)".to_string());
                                } else {
                                    hand.status = HandStatus::Settled { winner: "Player".to_string() };
                                    results.push("Player".to_string());
                                }
                            } else {
                                if p_score == 21 && hand.cards.len() == 2 && game.dealer_hand.len() != 2 {
                                    hand.status = HandStatus::Settled { winner: "Player (Blackjack)".to_string() };
                                    results.push("Player (Blackjack)".to_string());
                                } else if p_score == 21 && hand.cards.len() != 2 && game.dealer_hand.len() == 2 {
                                    hand.status = HandStatus::Settled { winner: "Dealer".to_string() };
                                    results.push("Dealer".to_string());
                                } else {
                                    hand.status = HandStatus::Settled { winner: "Push".to_string() };
                                    results.push("Push".to_string());
                                }
                            }
                        }
                    } else {
                        all_settled = false;
                        break;
                    }
                }

                if all_settled {
                    GameStatus::Settled {
                        winner: results.join(", "),
                    }
                } else {
                    let card_to_reveal = game.last_card_index;
                    game.last_card_index += 1;
                    GameStatus::WaitingForReveal {
                        reveal_requests: vec![card_to_reveal],
                        next_status: Box::new(GameStatus::DealerTurn),
                    }
                }
            }
            _ => *next_status,
        };
    } else {
        game.status = GameStatus::WaitingForReveal {
            reveal_requests,
            next_status,
        };
    }

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "submit_reveal")
        .add_attribute("card_index", card_index.to_string())
        .add_attribute("card_value", card_value.to_string()))
}

pub fn execute_claim_timeout(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let game = GAMES.load(deps.storage, &info.sender)?;

    // Define timeout threshold (e.g., 5 minutes = 300 seconds)
    const TIMEOUT_SECONDS: u64 = 300;

    let current_time = env.block.time.seconds();
    let time_elapsed = current_time.saturating_sub(game.last_action_timestamp);

    if time_elapsed < TIMEOUT_SECONDS {
        return Err(ContractError::Std(StdError::msg(format!(
            "Timeout not reached. Elapsed: {}s, Required: {}s",
            time_elapsed, TIMEOUT_SECONDS
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

    // Remove game from storage
    GAMES.remove(deps.storage, &info.sender);

    // Build response with refund if player wins
    let mut response = Response::new()
        .add_attribute("action", "claim_timeout")
        .add_attribute("winner", winner)
        .add_attribute("elapsed_seconds", time_elapsed.to_string());

    if refund_amount > Uint128::zero() {
        response = response.add_message(cosmwasm_std::BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![cosmwasm_std::Coin {
                denom: "uatom".to_string(), // Replace with actual denom
                amount: refund_amount.into(),
            }],
        });
    }

    Ok(response)
}
