// TODO: Add insurance logic
mod card;
#[cfg(feature = "edge")]
mod edge;
mod game_state;
mod hand;
mod rules;

pub use card::Card;
#[cfg(feature = "edge")]
pub use edge::{EdgeCalculator, EdgeResult};
pub use game_state::{GamePhase, GameState, Spot, TurnOwner};
pub use hand::{
    calculate_hand_value, can_split_cards, is_blackjack, is_busted, is_soft_hand, Hand, HandOutcome,
};
pub use rules::{DoubleRestriction, GameRules, PayoutRatio};
