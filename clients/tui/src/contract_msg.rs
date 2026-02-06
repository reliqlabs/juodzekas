/// Contract message types - mirrors contracts/juodzekas/src/msg.rs
/// Used for serializing messages to send to the smart contract

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Uint128(pub String);

impl Uint128 {
    pub fn new(value: u128) -> Self {
        Self(value.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Binary(pub String);

impl Binary {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(base64::encode(bytes))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    JoinGame {
        bet: Uint128,
        public_key: Binary,
    },
    SubmitShuffle {
        shuffled_deck: Vec<Binary>,
        proof: Binary,
    },
    Hit {},
    Stand {},
    DoubleDown {},
    Split {},
    Surrender {},
    SubmitReveal {
        card_index: u32,
        partial_decryption: Binary,
        proof: Binary,
    },
    ClaimTimeout {},
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetConfig {},
    GetGame { player: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameResponse {
    pub player: String,
    pub bet: Uint128,
    pub status: String,
    pub hands: Vec<HandResponse>,
    pub dealer_hand: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HandResponse {
    pub cards: Vec<u8>,
    pub bet: Uint128,
    pub status: String,
}

// Helper to encode messages as JSON
impl ExecuteMsg {
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl QueryMsg {
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
