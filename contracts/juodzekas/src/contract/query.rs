use cosmwasm_std::{to_json_binary, Binary, Deps, Env, Order, StdResult};
use crate::msg::{DealerBalanceResponse, DealerResponse, GameListItem, GameResponse, PendingRevealResponse, QueryMsg};
use crate::state::{Config, CONFIG, DEALER, DEALER_BALANCE, GAMES};

pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetGame { game_id } => to_json_binary(&query_game(deps, game_id)?),
        QueryMsg::ListGames { status_filter, limit, start_after } => to_json_binary(&query_list_games(deps, status_filter, limit, start_after)?),
        QueryMsg::GetDealerBalance {} => to_json_binary(&query_dealer_balance(deps)?),
        QueryMsg::GetDealer {} => to_json_binary(&query_dealer(deps)?),
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

    let pending_reveals = game
        .pending_reveals
        .into_iter()
        .map(|pr| PendingRevealResponse {
            card_index: pr.card_index,
            player_partial: pr.player_partial,
            dealer_partial: pr.dealer_partial,
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
        pending_reveals,
    })
}

fn query_list_games(deps: Deps, status_filter: Option<String>, limit: Option<u32>, start_after: Option<u64>) -> StdResult<Vec<GameListItem>> {
    let max_limit = limit.unwrap_or(30).min(100) as usize;
    let start = start_after.map(|id| cw_storage_plus::Bound::exclusive(id));

    let games: Result<Vec<GameListItem>, _> = GAMES
        .range(deps.storage, start, None, Order::Ascending)
        .filter_map(|item| {
            match item {
                Ok((game_id, game)) => {
                    let status_str = format!("{:?}", game.status);
                    if let Some(ref filter) = status_filter {
                        if !status_str.contains(filter) {
                            return None;
                        }
                    }
                    Some(Ok(GameListItem {
                        game_id,
                        dealer: game.dealer.to_string(),
                        player: game.player.to_string(),
                        status: status_str,
                        bet: game.bet,
                    }))
                }
                Err(e) => Some(Err(e)),
            }
        })
        .take(max_limit)
        .collect();

    games
}

fn query_dealer_balance(deps: Deps) -> StdResult<DealerBalanceResponse> {
    let balance = DEALER_BALANCE.load(deps.storage)?;
    Ok(DealerBalanceResponse { balance })
}

fn query_dealer(deps: Deps) -> StdResult<DealerResponse> {
    let dealer = DEALER.load(deps.storage)?;
    Ok(DealerResponse { dealer: dealer.to_string() })
}
