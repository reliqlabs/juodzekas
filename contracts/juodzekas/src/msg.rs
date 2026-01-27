use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Uint128};

use crate::state::{Config, DoubleDownRestriction};

#[cw_serde]
pub struct InstantiateMsg {
    pub min_bet: Uint128,
    pub max_bet: Uint128,
    pub bj_payout_permille: u64,
    pub insurance_payout_permille: u64,
    pub standard_payout_permille: u64,
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
pub enum ExecuteMsg {
    // Phase 1: Join and provide public key
    JoinGame { 
        bet: Uint128,
        public_key: Binary,
    },
    // Phase 2: Submit shuffle result and proof
    SubmitShuffle {
        shuffled_deck: Vec<Binary>,
        proof: Binary,
    },
    // Phase 3 & 4: Game actions
    Hit {},
    Stand {},
    DoubleDown {},
    Split {},
    Surrender {},
    // Phase 3, 4, 5: Submit reveal and proof for a card
    SubmitReveal {
        card_index: u32,
        partial_decryption: Binary,
        proof: Binary,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    GetConfig {},
    #[returns(GameResponse)]
    GetGame { player: String },
}

#[cw_serde]
pub struct GameResponse {
    pub player: String,
    pub bet: Uint128,
    pub status: String,
    pub hands: Vec<HandResponse>,
    pub dealer_hand: Vec<u8>,
}

#[cw_serde]
pub struct HandResponse {
    pub cards: Vec<u8>,
    pub bet: Uint128,
    pub status: String,
}
