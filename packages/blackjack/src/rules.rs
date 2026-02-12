use serde::{Deserialize, Serialize};

/// Restrictions on when doubling down is allowed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoubleRestriction {
    /// Can double on any two cards
    Any,
    /// Can only double on hard 9, 10, or 11
    Hard9_10_11,
    /// Can only double on hard 10 or 11
    Hard10_11,
}

/// Blackjack payout multiplier as a ratio
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutRatio {
    pub numerator: u16,
    pub denominator: u16,
}

impl PayoutRatio {
    pub const THREE_TO_TWO: Self = Self {
        numerator: 3,
        denominator: 2,
    };
    pub const SIX_TO_FIVE: Self = Self {
        numerator: 6,
        denominator: 5,
    };
    pub const ONE_TO_ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    pub fn new(numerator: u16, denominator: u16) -> Result<Self, &'static str> {
        if denominator == 0 {
            return Err("Denominator cannot be zero");
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    pub fn calculate_payout(&self, bet: u128) -> u128 {
        (bet * self.numerator as u128) / self.denominator as u128
    }
}

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

    /// Restriction on when doubling is allowed based on hand value
    pub double_restriction: DoubleRestriction,

    /// Allow re-splitting (up to max_splits total)
    pub allow_resplit: bool,

    /// Maximum number of splits allowed per hand
    pub max_splits: u8,

    /// Can split aces multiple times
    pub resplit_aces: bool,

    /// Dealer peeks for blackjack with Ace or 10 up
    pub dealer_peeks: bool,

    /// Blackjack payout multiplier (commonly 3:2 or 6:5)
    pub blackjack_payout: PayoutRatio,

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
            double_restriction: DoubleRestriction::Any,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_payout: PayoutRatio::THREE_TO_TWO,
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
            double_restriction: DoubleRestriction::Any,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: false,
            blackjack_payout: PayoutRatio::THREE_TO_TWO,
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
            double_restriction: DoubleRestriction::Any,
            allow_resplit: true,
            max_splits: 3,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_payout: PayoutRatio::THREE_TO_TWO,
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
            double_restriction: DoubleRestriction::Hard10_11,
            allow_resplit: false,
            max_splits: 0,
            resplit_aces: false,
            dealer_peeks: true,
            blackjack_payout: PayoutRatio::SIX_TO_FIVE,
            num_decks: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payout_ratio_three_to_two() {
        let ratio = PayoutRatio::THREE_TO_TWO;
        assert_eq!(ratio.calculate_payout(100), 150);
        assert_eq!(ratio.calculate_payout(10), 15);
        assert_eq!(ratio.calculate_payout(50), 75);
    }

    #[test]
    fn test_payout_ratio_six_to_five() {
        let ratio = PayoutRatio::SIX_TO_FIVE;
        assert_eq!(ratio.calculate_payout(100), 120);
        assert_eq!(ratio.calculate_payout(10), 12);
        assert_eq!(ratio.calculate_payout(50), 60);
    }

    #[test]
    fn test_payout_ratio_one_to_one() {
        let ratio = PayoutRatio::ONE_TO_ONE;
        assert_eq!(ratio.calculate_payout(100), 100);
        assert_eq!(ratio.calculate_payout(25), 25);
    }

    #[test]
    fn test_payout_ratio_custom() {
        let ratio = PayoutRatio::new(2, 1).unwrap();
        assert_eq!(ratio.calculate_payout(100), 200);
    }

    #[test]
    fn test_payout_ratio_zero_denominator() {
        assert!(PayoutRatio::new(3, 0).is_err());
    }

    #[test]
    fn test_game_rules_default_payout() {
        let rules = GameRules::default();
        assert_eq!(rules.blackjack_payout, PayoutRatio::THREE_TO_TWO);
    }

    #[test]
    fn test_game_rules_single_deck_payout() {
        let rules = GameRules::single_deck();
        assert_eq!(rules.blackjack_payout, PayoutRatio::SIX_TO_FIVE);
    }
}
