use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct Config {
    pub min_bet: Uint128,
    pub max_bet: Uint128,
    pub bj_payout_permille: u64,       // e.g., 1500 for 1.5x (3:2)
    pub insurance_payout_permille: u64, // e.g., 2000 for 2x
    pub standard_payout_permille: u64,  // e.g., 1000 for 1x
    pub dealer_hits_soft_17: bool,
    pub dealer_peeks: bool,
    pub double_down_restriction: DoubleDownRestriction,
    pub max_splits: u32,
    pub can_split_aces: bool,
    pub can_hit_split_aces: bool,
    pub surrender_allowed: bool,
    pub shuffle_vk_id: String,
    pub reveal_vk_id: String,
}

#[cw_serde]
pub enum DoubleDownRestriction {
    Any,
    Hard9_10_11,
    Hard10_11,
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
    pub hands: Vec<Hand>,
    pub current_hand_index: u32,
    pub dealer_hand: Vec<u8>,
    pub status: GameStatus,
    pub last_card_index: u32,
}

#[cw_serde]
pub struct Hand {
    pub cards: Vec<u8>,
    pub bet: Uint128,
    pub status: HandStatus,
}

#[cw_serde]
pub enum HandStatus {
    Active,
    Stood,
    Busted,
    Doubled,
    Surrendered,
    Settled { winner: String },
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const GAMES: Map<&Addr, GameSession> = Map::new("games");
