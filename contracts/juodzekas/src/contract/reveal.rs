use cosmwasm_std::{BankMsg, Binary, Coin, DepsMut, Env, MessageInfo, Response, StdError, Uint128};
use crate::error::ContractError;
use crate::state::{GameSession, GameStatus, HandStatus, PendingReveal, CONFIG, GAMES};
use crate::zk::xion_zk_verify;

/// Handle submission of partial decryption from player or dealer
/// Both parties must submit before card is revealed
#[allow(clippy::too_many_arguments)]
pub fn execute_submit_reveal(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    game_id: u64,
    card_index: u32,
    partial_decryption: Binary,
    proof: Binary,
    public_inputs: Vec<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Load game by ID
    let mut game = GAMES.load(deps.storage, game_id)?;

    // Determine if sender is player or dealer
    let is_player = info.sender == game.player;
    let is_dealer = info.sender == game.dealer;

    if !is_player && !is_dealer {
        return Err(ContractError::Std(StdError::msg("Sender is not part of this game")));
    }

    // Verify we're in WaitingForReveal status
    let (reveal_requests, next_status) = match &game.status {
        GameStatus::WaitingForReveal {
            reveal_requests,
            next_status,
        } => (reveal_requests.clone(), next_status.clone()),
        _ => {
            return Err(ContractError::Std(StdError::msg(format!(
                "Not waiting for reveal. Current status: {:?}",
                game.status
            ))))
        }
    };

    // Verify this card is expected
    if !reveal_requests.contains(&card_index) {
        return Err(ContractError::Std(StdError::msg(format!(
            "Card {card_index} not in pending reveals: {reveal_requests:?}"
        ))));
    }

    // Verify the ZK proof
    let verified = xion_zk_verify(deps.as_ref(), &config.reveal_vk_id, proof, public_inputs)?;
    if !verified {
        return Err(ContractError::Std(StdError::msg("Invalid reveal proof")));
    }

    // Find or create pending reveal for this card
    let pending_reveal_pos = game
        .pending_reveals
        .iter()
        .position(|pr| pr.card_index == card_index);

    let mut pending_reveal = if let Some(pos) = pending_reveal_pos {
        game.pending_reveals.remove(pos)
    } else {
        PendingReveal {
            card_index,
            player_partial: None,
            dealer_partial: None,
        }
    };

    // Store the partial decryption
    if is_player {
        if pending_reveal.player_partial.is_some() {
            return Err(ContractError::Std(StdError::msg("Player already revealed this card")));
        }
        pending_reveal.player_partial = Some(partial_decryption.clone());
    } else {
        if pending_reveal.dealer_partial.is_some() {
            return Err(ContractError::Std(StdError::msg("Dealer already revealed this card")));
        }
        pending_reveal.dealer_partial = Some(partial_decryption.clone());
    }

    // Check if both parties have submitted
    let both_revealed = pending_reveal.player_partial.is_some() && pending_reveal.dealer_partial.is_some();

    if both_revealed {
        // Combine partial decryptions to reveal the card
        // In Mental Poker with ElGamal: final_card = ciphertext.c1 - (player_partial + dealer_partial)
        // For simplicity, we use the first byte of player partial XOR dealer partial mod 52
        let player_bytes = pending_reveal.player_partial.as_ref().unwrap().as_slice();
        let dealer_bytes = pending_reveal.dealer_partial.as_ref().unwrap().as_slice();

        // Simple combination: XOR first bytes and mod 52
        let card_value = (player_bytes[0] ^ dealer_bytes[0]) % 52;

        // Add card to appropriate hand/dealer based on card_index
        let for_dealer = matches!(*next_status, GameStatus::DealerTurn);
        add_card_to_game(&mut game, card_index, card_value, for_dealer)?;

        // Remove this card from reveal_requests
        let remaining_requests: Vec<u32> = reveal_requests
            .iter()
            .copied()
            .filter(|&idx| idx != card_index)
            .collect();

        // Update game status
        if remaining_requests.is_empty() {
            // All cards revealed, transition to next status
            game.status = determine_next_status(&mut game, &next_status, &config)?;
        } else {
            // Still waiting for more reveals
            game.status = GameStatus::WaitingForReveal {
                reveal_requests: remaining_requests,
                next_status,
            };
        }

        game.last_action_timestamp = env.block.time.seconds();

        // Check if game is settled and execute payouts
        let mut response = Response::new()
            .add_attribute("action", "submit_reveal")
            .add_attribute("sender", if is_player { "player" } else { "dealer" })
            .add_attribute("card_index", card_index.to_string())
            .add_attribute("card_value", card_value.to_string())
            .add_attribute("both_revealed", "true");

        if matches!(game.status, GameStatus::Settled { .. }) {
            response = execute_payouts(&game, &config, response)?;
        }
        GAMES.save(deps.storage, game_id, &game)?;

        Ok(response)
    } else {
        // Only one party has submitted, wait for the other
        game.pending_reveals.push(pending_reveal);
        game.last_action_timestamp = env.block.time.seconds();
        GAMES.save(deps.storage, game_id, &game)?;

        Ok(Response::new()
            .add_attribute("action", "submit_reveal")
            .add_attribute("sender", if is_player { "player" } else { "dealer" })
            .add_attribute("card_index", card_index.to_string())
            .add_attribute("both_revealed", "false")
            .add_attribute("waiting_for", if is_player { "dealer" } else { "player" }))
    }
}

/// Add revealed card to the appropriate hand
fn add_card_to_game(game: &mut GameSession, card_index: u32, card_value: u8, for_dealer: bool) -> Result<(), ContractError> {
    // Initial deal: cards 0-1 go to player, cards 2-3 go to dealer
    if card_index < 2 {
        if game.hands.is_empty() {
            return Err(ContractError::Std(StdError::msg("No player hands")));
        }
        game.hands[0].cards.push(card_value);
    } else if card_index == 2 || card_index == 3 {
        game.dealer_hand.push(card_value);
    } else if for_dealer {
        // Dealer hit cards
        game.dealer_hand.push(card_value);
    } else {
        // Player hit/double/split cards
        let hand_idx = game.current_hand_index as usize;
        if hand_idx >= game.hands.len() {
            return Err(ContractError::Std(StdError::msg("Invalid hand index")));
        }
        game.hands[hand_idx].cards.push(card_value);
    }
    Ok(())
}

/// Determine the next game status after all reveals complete
fn determine_next_status(
    game: &mut GameSession,
    next_status: &GameStatus,
    config: &crate::state::Config,
) -> Result<GameStatus, ContractError> {
    match next_status {
        GameStatus::PlayerTurn => {
            // Check player hand status
            let hand_idx = game.current_hand_index as usize;
            if hand_idx >= game.hands.len() {
                return Err(ContractError::Std(StdError::msg("Invalid hand index")));
            }

            let hand = &game.hands[hand_idx];
            let p_score = crate::contract::calculate_score(&hand.cards);

            if p_score > 21 {
                // Busted - move to next hand or dealer turn
                if hand_idx + 1 < game.hands.len() {
                    Ok(GameStatus::PlayerTurn)
                } else {
                    Ok(GameStatus::WaitingForReveal {
                        reveal_requests: vec![3],
                        next_status: Box::new(GameStatus::DealerTurn),
                    })
                }
            } else if p_score == 21 {
                // 21 - auto-stand, move to next hand or dealer
                if hand_idx + 1 < game.hands.len() {
                    Ok(GameStatus::PlayerTurn)
                } else {
                    Ok(GameStatus::WaitingForReveal {
                        reveal_requests: vec![3],
                        next_status: Box::new(GameStatus::DealerTurn),
                    })
                }
            } else if matches!(hand.status, HandStatus::Doubled) {
                // Doubled - auto-stand after one card
                if hand_idx + 1 < game.hands.len() {
                    Ok(GameStatus::PlayerTurn)
                } else {
                    Ok(GameStatus::WaitingForReveal {
                        reveal_requests: vec![3],
                        next_status: Box::new(GameStatus::DealerTurn),
                    })
                }
            } else {
                Ok(GameStatus::PlayerTurn)
            }
        }
        GameStatus::DealerTurn => {
            let d_score = crate::contract::calculate_score(&game.dealer_hand);

            // Check if dealer needs to hit
            if d_score < 17 {
                // Dealer must hit — allocate next card index
                let card_to_reveal = game.last_card_index;
                game.last_card_index += 1;
                Ok(GameStatus::WaitingForReveal {
                    reveal_requests: vec![card_to_reveal],
                    next_status: Box::new(GameStatus::DealerTurn),
                })
            } else if d_score == 17 && config.dealer_hits_soft_17 {
                // Check if soft 17
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
                    // Soft 17, dealer hits — allocate next card index
                    let card_to_reveal = game.last_card_index;
                    game.last_card_index += 1;
                    Ok(GameStatus::WaitingForReveal {
                        reveal_requests: vec![card_to_reveal],
                        next_status: Box::new(GameStatus::DealerTurn),
                    })
                } else {
                    // Hard 17, settle
                    settle_game(game, d_score)
                }
            } else if d_score > 21 || d_score >= 17 {
                // Dealer busted or stands, settle game
                settle_game(game, d_score)
            } else {
                Ok(GameStatus::DealerTurn)
            }
        }
        _ => Ok(next_status.clone()),
    }
}

/// Settle the game and determine winners
fn settle_game(game: &GameSession, d_score: u8) -> Result<GameStatus, ContractError> {
    let mut results = vec![];

    for hand in &game.hands {
        let result = match hand.status {
            HandStatus::Busted => "Dealer",
            HandStatus::Surrendered => "Surrendered",
            HandStatus::Settled { ref winner } => winner.as_str(),
            _ => {
                let p_score = crate::contract::calculate_score(&hand.cards);
                if d_score > 21 {
                    "Player"
                } else if p_score > d_score {
                    if p_score == 21 && hand.cards.len() == 2 {
                        "Player (Blackjack)"
                    } else {
                        "Player"
                    }
                } else if p_score < d_score {
                    "Dealer"
                } else {
                    // Push
                    if p_score == 21 && hand.cards.len() == 2 && game.dealer_hand.len() != 2 {
                        "Player (Blackjack)"
                    } else if p_score == 21 && hand.cards.len() != 2 && game.dealer_hand.len() == 2 {
                        "Dealer"
                    } else {
                        "Push"
                    }
                }
            }
        };
        results.push(result.to_string());
    }

    Ok(GameStatus::Settled {
        winner: results.join(", "),
    })
}

/// Execute payouts based on game results
fn execute_payouts(
    game: &GameSession,
    config: &crate::state::Config,
    mut response: Response,
) -> Result<Response, ContractError> {
    let mut player_winnings = Uint128::zero();

    for hand in &game.hands {
        let winner = match &hand.status {
            HandStatus::Settled { winner } => winner.as_str(),
            _ => continue,
        };

        match winner {
            "Player (Blackjack)" => {
                // Blackjack pays 3:2 (or configured ratio)
                let payout = config.blackjack_payout.calculate_payout(hand.bet);
                player_winnings = player_winnings.checked_add(hand.bet + payout)
                    .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            }
            "Player" => {
                // Standard win pays 1:1
                let payout = config.standard_payout.calculate_payout(hand.bet);
                player_winnings = player_winnings.checked_add(hand.bet + payout)
                    .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            }
            "Push" => {
                // Push returns original bet
                player_winnings = player_winnings.checked_add(hand.bet)
                    .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            }
            "Surrendered" => {
                // Surrender returns half bet
                let refund = hand.bet.checked_div(Uint128::new(2))
                    .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
                player_winnings = player_winnings.checked_add(refund)
                    .map_err(|e| ContractError::Std(StdError::msg(e.to_string())))?;
            }
            "Dealer" => {
                // Dealer wins, player loses bet (no payout to player)
            }
            _ => {}
        }
    }

    // Send winnings to player if any
    if player_winnings > Uint128::zero() {
        response = response.add_message(BankMsg::Send {
            to_address: game.player.to_string(),
            amount: vec![Coin {
                denom: config.denom.clone(),
                amount: cosmwasm_std::Uint256::from(player_winnings),
            }],
        });
    }

    response = response
        .add_attribute("player_winnings", player_winnings.to_string())
        .add_attribute("game_settled", "true");

    Ok(response)
}
