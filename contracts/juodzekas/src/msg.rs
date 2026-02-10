use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Uint128};

pub use crate::state::{Config, DoubleRestriction, PayoutRatio};

#[cw_serde]
pub struct InstantiateMsg {
    pub denom: String,
    pub min_bet: Uint128,
    pub max_bet: Uint128,
    pub blackjack_payout: PayoutRatio,
    pub insurance_payout: PayoutRatio,
    pub standard_payout: PayoutRatio,
    pub dealer_hits_soft_17: bool,
    pub dealer_peeks: bool,
    pub double_restriction: DoubleRestriction,
    pub max_splits: u32,
    pub can_split_aces: bool,
    pub can_hit_split_aces: bool,
    pub surrender_allowed: bool,
    pub shuffle_vk_id: String,
    pub reveal_vk_id: String,
    /// Timeout in seconds for inactivity claims and settled game cleanup. Defaults to 3600 (1 hour).
    pub timeout_seconds: Option<u64>,
}

#[cw_serde]
pub enum ExecuteMsg {
    // Phase 1: Dealer creates game and submits initial shuffle + proof
    CreateGame {
        public_key: Binary,
        shuffled_deck: Vec<Binary>,
        proof: Binary,
        public_inputs: Vec<String>,
    },
    // Phase 2: Player joins with bet, public key, re-shuffle + proof
    // Auto-finds the first WaitingForPlayerJoin game
    JoinGame {
        bet: Uint128,
        public_key: Binary,
        shuffled_deck: Vec<Binary>,
        proof: Binary,
        public_inputs: Vec<String>,
    },
    // Phase 3 & 4: Game actions
    Hit { game_id: u64 },
    Stand { game_id: u64 },
    DoubleDown { game_id: u64 },
    Split { game_id: u64 },
    Surrender { game_id: u64 },
    // Phase 3, 4, 5: Submit reveal and proof for a card
    SubmitReveal {
        game_id: u64,
        card_index: u32,
        partial_decryption: Binary,
        proof: Binary,
        public_inputs: Vec<String>,
    },
    // Timeout claim: if opponent doesn't act, claim funds
    ClaimTimeout { game_id: u64 },
    // Cancel an unjoined game and return bankroll
    CancelGame { game_id: u64 },
    // Permissionless cleanup of settled games past timeout
    SweepSettled { game_ids: Vec<u64> },
    // Deposit additional bankroll
    DepositBankroll {},
    // Withdraw dealer bankroll balance
    WithdrawBankroll { amount: Option<Uint128> },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    GetConfig {},
    #[returns(GameResponse)]
    GetGame { game_id: u64 },
    #[returns(Vec<GameListItem>)]
    ListGames { status_filter: Option<String> },
    #[returns(DealerBalanceResponse)]
    GetDealerBalance {},
    #[returns(DealerResponse)]
    GetDealer {},
}

#[cw_serde]
pub struct PendingRevealResponse {
    pub card_index: u32,
    pub player_partial: Option<Binary>,
    pub dealer_partial: Option<Binary>,
}

#[cw_serde]
pub struct GameResponse {
    pub player: String,
    pub dealer: String,
    pub bet: Uint128,
    pub status: String,
    pub hands: Vec<HandResponse>,
    pub dealer_hand: Vec<u8>,
    pub player_pubkey: Binary,
    pub dealer_pubkey: Binary,
    pub deck: Vec<Binary>,
    pub player_shuffled_deck: Option<Vec<Binary>>,
    pub pending_reveals: Vec<PendingRevealResponse>,
}

#[cw_serde]
pub struct HandResponse {
    pub cards: Vec<u8>,
    pub bet: Uint128,
    pub status: String,
}

#[cw_serde]
pub struct DealerBalanceResponse {
    pub balance: Uint128,
}

#[cw_serde]
pub struct DealerResponse {
    pub dealer: String,
}

#[cw_serde]
pub struct GameListItem {
    pub game_id: u64,
    pub dealer: String,
    pub player: String,
    pub status: String,
    pub bet: Uint128,
}
