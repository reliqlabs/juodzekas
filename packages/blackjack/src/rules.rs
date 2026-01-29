use serde::{Deserialize, Serialize};

/// Configurable blackjack game rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRules {
    /// Dealer hits on soft 17
    pub dealer_hits_soft_17: bool,

    /// Allow surrender
    pub allow_surrender: bool,

    /// Allow late surrender (after dealer checks for blackjack)
    pub late_surrender: bool,

    /// Allow doubling after split
    pub double_after_split: bool,

    /// Allow re-splitting (up to max_splits total)
    pub allow_resplit: bool,

    /// Maximum number of splits allowed per hand
    pub max_splits: u8,

    /// Can split aces multiple times
    pub resplit_aces: bool,

    /// Dealer peeks for blackjack with Ace or 10 up
    pub dealer_peeks: bool,

    /// Blackjack pays 3:2 (true) or 6:5 (false)
    pub blackjack_pays_3_to_2: bool,

    /// Number of decks in the shoe
    pub num_decks: u8,
}

impl Default for GameRules {
    fn default() -> Self {
        // Standard Las Vegas rules
        Self {
            dealer_hits_soft_17: false,
            allow_surrender: true,
            late_surrender: true,
            double_after_split: true,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_pays_3_to_2: true,
            num_decks: 6,
        }
    }
}

impl GameRules {
    /// European rules (dealer doesn't peek, no surrender)
    pub fn european() -> Self {
        Self {
            dealer_hits_soft_17: false,
            allow_surrender: false,
            late_surrender: false,
            double_after_split: false,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: false,
            blackjack_pays_3_to_2: true,
            num_decks: 6,
        }
    }

    /// Atlantic City rules
    pub fn atlantic_city() -> Self {
        Self {
            dealer_hits_soft_17: false,
            allow_surrender: true,
            late_surrender: true,
            double_after_split: true,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_pays_3_to_2: true,
            num_decks: 8,
        }
    }

    /// Single deck rules (often found in casinos, but with 6:5 blackjack)
    pub fn single_deck() -> Self {
        Self {
            dealer_hits_soft_17: true,
            allow_surrender: false,
            late_surrender: false,
            double_after_split: false,
            allow_resplit: false,
            max_splits: 0,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_pays_3_to_2: false, // Often 6:5
            num_decks: 1,
        }
    }
}
