use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub min_bet: Uint128,
    pub shuffle_vk_id: String,
    pub reveal_vk_id: String,
}

#[cw_serde]
pub enum GameStatus {
    WaitingForShuffle,
    WaitingForReveal {
        reveal_requests: Vec<u32>, // Indices of cards to be revealed
        next_status: Box<GameStatus>,
    },
    PlayerTurn,
    DealerTurn,
    Settled {
        winner: String,
    },
}

#[cw_serde]
pub struct GameSession {
    pub player: Addr,
    pub bet: Uint128,
    pub player_pubkey: Binary,
    pub dealer_pubkey: Binary,
    pub deck: Vec<Binary>, // Encrypted cards
    pub player_hand: Vec<u8>,
    pub dealer_hand: Vec<u8>,
    pub status: GameStatus,
    pub last_card_index: u32,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const GAMES: Map<&Addr, GameSession> = Map::new("games");
