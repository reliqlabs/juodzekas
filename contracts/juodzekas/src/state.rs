use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Uint128};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct PayoutRatio {
    pub numerator: u16,
    pub denominator: u16,
}

impl PayoutRatio {
    pub fn calculate_payout(&self, bet: Uint128) -> Uint128 {
        // numerator/denominator are u16 (max 65535) and bet is bounded by max_bet,
        // so overflow is practically impossible. But panic on overflow instead of
        // silently returning a wrong value.
        bet.checked_mul(Uint128::new(self.numerator as u128))
            .expect("payout multiplication overflow")
            .checked_div(Uint128::new(self.denominator as u128))
            .expect("payout division by zero")
    }
}

#[cw_serde]
pub enum DoubleRestriction {
    Any,
    Hard9_10_11,
    Hard10_11,
}

#[cw_serde]
pub struct Config {
    pub denom: String,
    pub min_bet: Uint128,
    pub max_bet: Uint128,
    pub blackjack_payout: PayoutRatio,     // e.g., 3:2 or 6:5
    pub insurance_payout: PayoutRatio,     // e.g., 2:1
    pub standard_payout: PayoutRatio,      // e.g., 1:1
    pub dealer_hits_soft_17: bool,
    pub dealer_peeks: bool,
    pub double_restriction: DoubleRestriction,
    pub max_splits: u32,
    pub can_split_aces: bool,
    pub can_hit_split_aces: bool,
    pub surrender_allowed: bool,
    pub shuffle_vk_id: String,
    pub reveal_vk_id: String,
    pub timeout_seconds: u64,
}

#[cw_serde]
pub enum TurnOwner {
    Player,
    Dealer,
    None,
}

#[cw_serde]
pub enum GameStatus {
    WaitingForPlayerJoin,
    WaitingForReveal {
        reveal_requests: Vec<u32>, // Indices of cards to be revealed
        next_status: Box<GameStatus>,
    },
    OfferingInsurance,
    PlayerTurn,
    DealerTurn,
    Settled {
        winner: String,
    },
}

#[cw_serde]
pub struct GameSession {
    pub player: Addr,
    pub dealer: Addr,
    pub bet: Uint128,
    pub bankroll: Uint128,
    pub player_pubkey: Binary,
    pub dealer_pubkey: Binary,
    pub deck: Vec<Binary>, // Encrypted cards
    pub player_shuffled_deck: Option<Vec<Binary>>, // Player's shuffle before dealer re-shuffles
    pub hands: Vec<Hand>,
    pub current_hand_index: u32,
    pub dealer_hand: Vec<u8>,
    pub status: GameStatus,
    pub current_turn: TurnOwner,
    pub last_action_timestamp: u64, // Timestamp of last action for timeout tracking
    pub last_card_index: u32,
    pub pending_reveals: Vec<PendingReveal>, // Track partial decryptions from both parties
    pub dealer_peeked: bool,
    pub insurance_bet: Option<Uint128>,
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

#[cw_serde]
pub struct PendingReveal {
    pub card_index: u32,
    pub player_partial: Option<Binary>,
    pub dealer_partial: Option<Binary>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const GAME_COUNTER: Item<u64> = Item::new("game_counter");
pub const GAMES: Map<u64, GameSession> = Map::new("games");
pub const DEALER: Item<Addr> = Item::new("dealer");
pub const DEALER_BALANCE: Item<Uint128> = Item::new("dealer_balance");
