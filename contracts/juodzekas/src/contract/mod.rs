pub mod instantiate;
pub mod execute;
pub mod query;

// Unit tests removed - use integration tests in tests/integration.rs instead
// since MockQuerier doesn't support gRPC queries needed for ZK verification

pub use crate::contract::instantiate::instantiate;
pub use crate::contract::execute::execute;
pub use crate::contract::query::query;

/// Calculates the Blackjack score for a hand.
/// Handles Aces as 1 or 11 to maximize the score without busting.
pub fn calculate_score(hand: &[u8]) -> u8 {
    let mut score = 0;
    let mut aces = 0;
    for &card in hand {
        let val = (card % 13) + 1;
        if val == 1 {
            aces += 1;
            score += 11;
        } else if val > 10 {
            score += 10;
        } else {
            score += val;
        }
    }
    while score > 21 && aces > 0 {
        score -= 10;
        aces -= 1;
    }
    score
}
