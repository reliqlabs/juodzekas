use cosmwasm_std::{to_json_binary, Addr, Binary, Deps, Env, StdResult};
use crate::msg::{GameResponse, QueryMsg};
use crate::state::{Config, CONFIG, GAMES};

pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetConfig {} => to_json_binary(&query_config(deps)?),
        QueryMsg::GetGame { player } => to_json_binary(&query_game(deps, player)?),
    }
}

fn query_config(deps: Deps) -> StdResult<Config> {
    let config = CONFIG.load(deps.storage)?;
    Ok(config)
}

fn query_game(deps: Deps, player: String) -> StdResult<GameResponse> {
    let player_addr = if cfg!(test) {
        Addr::unchecked(player)
    } else {
        deps.api.addr_validate(&player)?
    };
    let game = GAMES.load(deps.storage, &player_addr)?;

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
        bet: game.bet,
        status: format!("{:?}", game.status),
        hands,
        dealer_hand: game.dealer_hand,
    })
}
