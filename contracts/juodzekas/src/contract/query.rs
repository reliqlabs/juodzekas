use cosmwasm_std::{to_json_binary, Binary, Deps, Env, Order, StdResult};
use crate::msg::{GameListItem, GameResponse, QueryMsg};
use crate::state::{Config, CONFIG, GAMES};

pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetGame { game_id } => to_json_binary(&query_game(deps, game_id)?),
        QueryMsg::ListGames { status_filter } => to_json_binary(&query_list_games(deps, status_filter)?),
    }
}

fn query_config(deps: Deps) -> StdResult<Config> {
    let config = CONFIG.load(deps.storage)?;
    Ok(config)
}

fn query_game(deps: Deps, game_id: u64) -> StdResult<GameResponse> {
    let game = GAMES.load(deps.storage, game_id)?;

    let hands = game
        .hands
        .into_iter()
        .map(|h| crate::msg::HandResponse {
            cards: h.cards,
            bet: h.bet,
            status: format!("{:?}", h.status),
        })
        .collect();

    Ok(GameResponse {
        player: game.player.to_string(),
        dealer: game.dealer.to_string(),
        bet: game.bet,
        status: format!("{:?}", game.status),
        hands,
        dealer_hand: game.dealer_hand,
        player_pubkey: game.player_pubkey,
        dealer_pubkey: game.dealer_pubkey,
        deck: game.deck,
        player_shuffled_deck: game.player_shuffled_deck,
    })
}

fn query_list_games(deps: Deps, status_filter: Option<String>) -> StdResult<Vec<GameListItem>> {
    let games: Result<Vec<GameListItem>, _> = GAMES
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (game_id, game) = item?;
            let status_str = format!("{:?}", game.status);

            // Apply status filter if provided
            if let Some(ref filter) = status_filter {
                if !status_str.contains(filter) {
                    return Ok(None);
                }
            }

            Ok(Some(GameListItem {
                game_id,
                dealer: game.dealer.to_string(),
                player: game.player.to_string(),
                status: status_str,
                bet: game.bet,
            }))
        })
        .filter_map(|r| r.transpose())
        .collect();

    games
}
