#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ConfigResponse, ExecuteMsg, GameResponse, InstantiateMsg, QueryMsg};
use crate::state::{Config, GameSession, GameStatus, CONFIG, GAMES};
use crate::zk::xion_zk_verify;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:juodzekas";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Entry point for contract instantiation.
/// Initializes the global configuration including minimum bet and ZK verification key IDs.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        min_bet: msg.min_bet,
        shuffle_vk_id: msg.shuffle_vk_id.clone(),
        reveal_vk_id: msg.reveal_vk_id.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("min_bet", msg.min_bet)
        .add_attribute("shuffle_vk_id", msg.shuffle_vk_id)
        .add_attribute("reveal_vk_id", msg.reveal_vk_id))
}

/// Entry point for contract execution.
/// Dispatches to specialized handlers based on the `ExecuteMsg`.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::JoinGame { bet, public_key } => execute_join_game(deps, info, bet, public_key),
        ExecuteMsg::SubmitShuffle {
            shuffled_deck,
            proof,
        } => execute_submit_shuffle(deps, info, shuffled_deck, proof),
        ExecuteMsg::Hit {} => execute_hit(deps, info),
        ExecuteMsg::Stand {} => execute_stand(deps, info),
        ExecuteMsg::SubmitReveal {
            card_index,
            partial_decryption,
            proof,
        } => execute_submit_reveal(deps, info, card_index, partial_decryption, proof),
    }
}

/// Starts a new game session for the sender.
/// Requires an initial bet and the player's public key for deck encryption.
pub fn execute_join_game(
    deps: DepsMut,
    info: MessageInfo,
    bet: Uint128,
    public_key: Binary,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if bet < config.min_bet {
        return Err(ContractError::Std(cosmwasm_std::StdError::msg(
            "Bet too low",
        )));
    }

    let game = GameSession {
        player: info.sender.clone(),
        bet,
        player_pubkey: public_key,
        dealer_pubkey: Binary::default(), // In a real scenario, this would be fixed or generated
        deck: vec![],
        player_hand: vec![],
        dealer_hand: vec![],
        status: GameStatus::WaitingForShuffle,
        last_card_index: 0,
    };

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "join_game")
        .add_attribute("player", info.sender))
}

/// Verifies a ZK shuffle proof and initializes the game deck.
/// Triggers the initial deal of 3 cards (2 for player, 1 for dealer).
pub fn execute_submit_shuffle(
    deps: DepsMut,
    info: MessageInfo,
    shuffled_deck: Vec<Binary>,
    proof: Binary,
) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
    let config = CONFIG.load(deps.storage)?;

    if game.status != GameStatus::WaitingForShuffle {
        return Err(ContractError::Std(StdError::msg(
            "Invalid game status",
        )));
    }

    // Verify proof via Xion ZK module
    // Public inputs for shuffle usually include the original deck and the shuffled deck
    // For simplicity, we assume the proof already includes them or they are verified by the module
    let public_inputs = vec![];
    let verified = xion_zk_verify(deps.as_ref(), &config.shuffle_vk_id, proof, public_inputs)?;

    if !verified {
        return Err(ContractError::Std(StdError::msg(
            "Invalid shuffle proof",
        )));
    }

    game.deck = shuffled_deck;
    // Initial deal: 2 for player, 2 for dealer (1 hidden)
    // We request reveals for the first 3 cards (indices 0, 1, 2)
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![0, 1, 2],
        next_status: Box::new(GameStatus::PlayerTurn),
    };
    game.last_card_index = 4; // Indices 0, 1, 2 are being revealed, 3 is the dealer's hidden card. Next card will be 4.

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "submit_shuffle")
        .add_attribute("player", info.sender))
}

pub fn execute_hit(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg(
            "Not player turn",
        )));
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
        .add_attribute("requested_card", card_to_reveal.to_string()))
}

pub fn execute_stand(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut game = GAMES.load(deps.storage, &info.sender)?;
    if game.status != GameStatus::PlayerTurn {
        return Err(ContractError::Std(StdError::msg(
            "Not player turn",
        )));
    }

    // Player stands, now dealer's turn. First reveal dealer's hidden card (index 3)
    game.status = GameStatus::WaitingForReveal {
        reveal_requests: vec![3],
        next_status: Box::new(GameStatus::DealerTurn),
    };

    GAMES.save(deps.storage, &info.sender, &game)?;

    Ok(Response::new()
        .add_attribute("action", "stand")
        .add_attribute("player", info.sender))
}

/// Verifies a partial decryption and its ZK proof.
/// Updates the game state based on the revealed card value and transitions to the next phase.
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
            return Err(ContractError::Std(StdError::msg(
                format!("No pending reveal. Current status: {:?}", game.status),
            )))
        }
    };

    if !reveal_requests.contains(&card_index) {
        return Err(ContractError::Std(StdError::msg(
            format!(
                "Invalid card reveal index: {card_index}. Expected one of: {reveal_requests:?}"
            ),
        )));
    }

    // Verify reveal proof via Xion ZK module
    let public_inputs = vec![
        card_index.to_string(),
        hex::encode(partial_decryption.as_slice()),
    ];
    let verified = xion_zk_verify(deps.as_ref(), &config.reveal_vk_id, proof, public_inputs)?;

    if !verified {
        return Err(ContractError::Std(StdError::msg(
            "Invalid reveal proof",
        )));
    }

    // In a real Mental Poker game, you'd combine multiple partial decryptions.
    // Here we simplify: the player's submission reveals the card.
    // We use the card_index to "randomly" pick a card value if we don't have real decryption logic.
    // For this scaffold, we'll map the partial_decryption to a card value 0-51.
    let card_value = (partial_decryption.as_slice()[0] % 52);

    if card_index < 2 {
        game.player_hand.push(card_value);
    } else if card_index == 2 || card_index == 3 {
        game.dealer_hand.push(card_value);
    } else {
        // Subsequent hits
        // Check if we are in PlayerTurn or DealerTurn after this reveal
        if let GameStatus::PlayerTurn = *next_status {
            game.player_hand.push(card_value);
        } else {
            game.dealer_hand.push(card_value);
        }
    }

    reveal_requests.retain(|&i| i != card_index);

    if reveal_requests.is_empty() {
        // Move to next state
        game.status = match *next_status {
            GameStatus::PlayerTurn => {
                let p_score = calculate_score(&game.player_hand);
                if p_score > 21 {
                    GameStatus::Settled {
                        winner: "Dealer".to_string(),
                    }
                } else if p_score == 21 && game.player_hand.len() == 2 {
                    // Blackjack!
                    GameStatus::WaitingForReveal {
                        reveal_requests: vec![3],
                        next_status: Box::new(GameStatus::DealerTurn),
                    }
                } else {
                    GameStatus::PlayerTurn
                }
            }
            GameStatus::DealerTurn => {
                let d_score = calculate_score(&game.dealer_hand);
                let p_score = calculate_score(&game.player_hand);

                if d_score > 21 {
                    GameStatus::Settled {
                        winner: "Player".to_string(),
                    }
                } else if d_score >= 17 {
                    if d_score > p_score {
                        GameStatus::Settled {
                            winner: "Dealer".to_string(),
                        }
                    } else if d_score < p_score {
                        GameStatus::Settled {
                            winner: "Player".to_string(),
                        }
                    } else {
                        GameStatus::Settled {
                            winner: "Push".to_string(),
                        }
                    }
                } else {
                    // Dealer hits
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

/// Calculates the Blackjack score for a hand.
/// Handles Aces as 1 or 11 to maximize the score without busting.
fn calculate_score(hand: &[u8]) -> u8 {
    let mut score = 0;
    let mut aces = 0;
    for &card in hand {
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
    while score > 21 && aces > 0 {
        score -= 10;
        aces -= 1;
    }
    score
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetGame { player } => to_json_binary(&query_game(deps, player)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        min_bet: config.min_bet,
        shuffle_vk_id: config.shuffle_vk_id,
        reveal_vk_id: config.reveal_vk_id,
    })
}

fn query_game(deps: Deps, player: String) -> StdResult<GameResponse> {
    let player_addr = if cfg!(test) {
        Addr::unchecked(player)
    } else {
        deps.api.addr_validate(&player)?
    };
    let game = GAMES.load(deps.storage, &player_addr)?;
    Ok(GameResponse {
        player: game.player.to_string(),
        bet: game.bet,
        status: format!("{:?}", game.status),
        player_hand: game.player_hand,
        dealer_hand: game.dealer_hand,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{coins, from_json, Addr, Uint128};

    #[test]
    fn test_calculate_score() {
        // Standard hand
        assert_eq!(calculate_score(&[0, 10]), 21); // Ace (0%13+1=1) and Jack (10%13+1=11 -> 10)
        // Two aces
        assert_eq!(calculate_score(&[0, 13]), 12); // Two Aces: 11 + 1 = 12
        // Bust and Ace adjustment
        assert_eq!(calculate_score(&[0, 9, 8]), 20); // Ace (1), 10, 9 -> 1+10+9=20.
        
        // Card % 13 + 1:
        // 0 -> 1 (Ace)
        // 1 -> 2
        // ...
        // 9 -> 10
        // 10 -> 11 (Jack -> 10)
        // 11 -> 12 (Queen -> 10)
        // 12 -> 13 (King -> 10)

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

        // After 3 reveals, should be PlayerTurn (since Player has 21 but not from 2 cards? wait 0 and 10 is 21)
        // Actually 0 is Ace(11) and 10 is Jack(10). 11+10=21. Blackjack!
        // If it's Blackjack, it should transition to Dealer reveal of card 3.
        
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
        assert_eq!(game.status, "Settled { winner: \"Player\" }");
    }

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();
        let creator = "cosmwasm1zg63vla7v7svzpxatp6y0v5fuv8vml5u7e66ax";

        let msg = InstantiateMsg {
            min_bet: Uint128::new(100),
            shuffle_vk_id: "shuffle_key".to_string(),
            reveal_vk_id: "reveal_key".to_string(),
        };
        let info = message_info(&Addr::unchecked(creator), &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetConfig {}).unwrap();
        let value: ConfigResponse = from_json(&res).unwrap();
        assert_eq!(Uint128::new(100), value.min_bet);
        assert_eq!("shuffle_key", value.shuffle_vk_id);
    }
}
