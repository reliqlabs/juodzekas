use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Uint128};

#[cw_serde]
pub struct InstantiateMsg {
    pub min_bet: Uint128,
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
    #[returns(ConfigResponse)]
    GetConfig {},
    #[returns(GameResponse)]
    GetGame { player: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub min_bet: Uint128,
    pub shuffle_vk_id: String,
    pub reveal_vk_id: String,
}

#[cw_serde]
pub struct GameResponse {
    pub player: String,
    pub bet: Uint128,
    pub status: String,
    pub player_hand: Vec<u8>,
    pub dealer_hand: Vec<u8>,
}
