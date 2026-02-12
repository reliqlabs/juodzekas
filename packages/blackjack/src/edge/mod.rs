use std::collections::HashMap;

use crate::{DoubleRestriction, GameRules};

/// Card counts by blackjack value index.
/// Index 0=Ace, 1=Two, 2=Three, ..., 8=Nine, 9=Ten/J/Q/K.
/// Single deck: [4,4,4,4,4,4,4,4,4,16].
type Shoe = [u8; 10];

/// Dealer outcome probability distribution.
/// [P(bust), P(17), P(18), P(19), P(20), P(21)]
type DealerProbs = [f64; 6];

/// Result of house edge calculation.
#[derive(Debug, Clone, Copy)]
pub struct EdgeResult {
    /// House edge as a fraction (e.g., 0.005 = 0.5%).
    /// Positive means house advantage.
    pub house_edge: f64,
    /// Expected return per unit bet for the player.
    pub expected_return: f64,
}

/// Combinatorial blackjack house edge calculator.
///
/// Computes exact house edge via enumeration of all possible deals and
/// composition-dependent optimal player decisions.
/// Uses split approximation (play one hand, multiply by 2).
pub struct EdgeCalculator {
    rules: GameRules,
    shoe: Shoe,
    total_cards: u16,
    dealer_cache: HashMap<(Shoe, u8, bool), DealerProbs>,
    dealer_upcard_cache: HashMap<(Shoe, u8), DealerProbs>,
    player_cache: HashMap<(Shoe, u8, bool, u8), f64>,
}

impl EdgeCalculator {
    pub fn new(rules: GameRules) -> Self {
        let shoe = Self::initial_shoe(rules.num_decks);
        let total_cards = shoe.iter().map(|&c| c as u16).sum();
        Self {
            rules,
            shoe,
            total_cards,
            dealer_cache: HashMap::new(),
            dealer_upcard_cache: HashMap::new(),
            player_cache: HashMap::new(),
        }
    }

    pub fn calculate(&mut self) -> EdgeResult {
        self.dealer_cache.clear();
        self.dealer_upcard_cache.clear();
        self.player_cache.clear();
        let expected_return = self.aggregate_ev();
        EdgeResult {
            house_edge: -expected_return,
            expected_return,
        }
    }

    // ── Shoe helpers ──

    fn initial_shoe(num_decks: u8) -> Shoe {
        let n = num_decks;
        [
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            4 * n,
            16 * n,
        ]
    }

    fn shoe_total(shoe: &Shoe) -> u16 {
        shoe.iter().map(|&c| c as u16).sum()
    }

    fn remove_card(shoe: &Shoe, idx: usize) -> Shoe {
        let mut s = *shoe;
        debug_assert!(s[idx] > 0);
        s[idx] -= 1;
        s
    }

    /// Blackjack value for a card at the given value index.
    /// Ace returns 1 (caller promotes to 11 via add_to_hand).
    fn card_value(idx: usize) -> u8 {
        if idx == 0 {
            1
        } else {
            (idx + 1) as u8
        }
    }

    /// Add a card to a hand, returning new (value, is_soft).
    fn add_to_hand(value: u8, is_soft: bool, card: u8) -> (u8, bool) {
        if card == 1 {
            if value + 11 <= 21 {
                (value + 11, true)
            } else {
                (value + 1, is_soft)
            }
        } else {
            let new_val = value + card;
            if new_val > 21 && is_soft {
                (new_val - 10, false)
            } else {
                (new_val, is_soft)
            }
        }
    }

    // ── Dealer outcome probabilities ──

    /// Recursive dealer probs from a given hand state (used for subsequent draws).
    fn dealer_probs(&mut self, shoe: Shoe, value: u8, is_soft: bool) -> DealerProbs {
        if let Some(&cached) = self.dealer_cache.get(&(shoe, value, is_soft)) {
            return cached;
        }

        let must_stand = if value >= 18 || value > 21 {
            true
        } else if value == 17 {
            !(is_soft && self.rules.dealer_hits_soft_17)
        } else {
            false
        };

        let result = if value > 21 {
            let mut r = [0.0; 6];
            r[0] = 1.0;
            r
        } else if must_stand {
            let mut r = [0.0; 6];
            r[(value - 16) as usize] = 1.0;
            r
        } else {
            let total = Self::shoe_total(&shoe);
            if total == 0 {
                let mut r = [0.0; 6];
                if value >= 17 {
                    r[(value - 16) as usize] = 1.0;
                } else {
                    r[0] = 1.0;
                }
                return r;
            }
            let mut r = [0.0; 6];
            for i in 0..10 {
                if shoe[i] == 0 {
                    continue;
                }
                let p = shoe[i] as f64 / total as f64;
                let cv = Self::card_value(i);
                let (nv, ns) = Self::add_to_hand(value, is_soft, cv);
                let new_shoe = Self::remove_card(&shoe, i);
                if nv > 21 {
                    r[0] += p;
                } else {
                    let sub = self.dealer_probs(new_shoe, nv, ns);
                    for j in 0..6 {
                        r[j] += p * sub[j];
                    }
                }
            }
            r
        };

        self.dealer_cache.insert((shoe, value, is_soft), result);
        result
    }

    /// Dealer probs starting from just the upcard.
    /// For peek games with BJ-possible upcard, conditions on the hole card
    /// NOT completing a blackjack (since peek already confirmed no BJ).
    fn dealer_probs_from_upcard(&mut self, shoe: Shoe, upcard_idx: u8) -> DealerProbs {
        let key = (shoe, upcard_idx);
        if let Some(&cached) = self.dealer_upcard_cache.get(&key) {
            return cached;
        }

        let cv = Self::card_value(upcard_idx as usize);
        let (d_val, d_soft) = Self::add_to_hand(0, false, cv);

        let needs_conditioning = self.rules.dealer_peeks && (cv == 1 || cv == 10);

        let result = if needs_conditioning {
            // Peek: hole card can't complete BJ.
            // Ace up → hole card can't be 10-value (idx 9)
            // 10 up → hole card can't be Ace (idx 0)
            let forbidden = if cv == 1 { 9 } else { 0 };
            let total = Self::shoe_total(&shoe);
            let forbidden_count = shoe[forbidden] as u16;
            let adj_total = total - forbidden_count;

            if adj_total == 0 {
                let mut r = [0.0; 6];
                if d_val >= 17 {
                    r[(d_val - 16) as usize] = 1.0;
                } else {
                    r[0] = 1.0;
                }
                return r;
            }

            let mut r = [0.0; 6];
            for i in 0..10 {
                if shoe[i] == 0 || i == forbidden {
                    continue;
                }
                let p = shoe[i] as f64 / adj_total as f64;
                let hcv = Self::card_value(i);
                let (nv, ns) = Self::add_to_hand(d_val, d_soft, hcv);
                let new_shoe = Self::remove_card(&shoe, i);
                if nv > 21 {
                    r[0] += p;
                } else {
                    // After hole card, dealer draws normally
                    let sub = self.dealer_probs(new_shoe, nv, ns);
                    for j in 0..6 {
                        r[j] += p * sub[j];
                    }
                }
            }
            r
        } else {
            // No conditioning needed — draw hole card from full shoe
            self.dealer_probs(shoe, d_val, d_soft)
        };

        self.dealer_upcard_cache.insert(key, result);
        result
    }

    // ── Stand EV ──

    fn stand_ev(player_value: u8, dp: &DealerProbs) -> f64 {
        let mut ev = dp[0]; // dealer bust → player wins
        for d in 17u8..=21 {
            let idx = (d - 16) as usize;
            if player_value > d {
                ev += dp[idx];
            } else if player_value < d {
                ev -= dp[idx];
            }
        }
        ev
    }

    // ── Hit-or-stand EV ──

    fn hit_or_stand_ev(
        &mut self,
        shoe: Shoe,
        player_value: u8,
        is_soft: bool,
        dealer_up: u8,
    ) -> f64 {
        let key = (shoe, player_value, is_soft, dealer_up);
        if let Some(&cached) = self.player_cache.get(&key) {
            return cached;
        }

        let dp = self.dealer_probs_from_upcard(shoe, dealer_up);
        let s_ev = Self::stand_ev(player_value, &dp);

        let total = Self::shoe_total(&shoe);
        let mut h_ev = 0.0;
        if total > 0 {
            for i in 0..10 {
                if shoe[i] == 0 {
                    continue;
                }
                let p = shoe[i] as f64 / total as f64;
                let cv = Self::card_value(i);
                let (nv, ns) = Self::add_to_hand(player_value, is_soft, cv);
                let new_shoe = Self::remove_card(&shoe, i);
                if nv > 21 {
                    h_ev += -p;
                } else {
                    h_ev += p * self.hit_or_stand_ev(new_shoe, nv, ns, dealer_up);
                }
            }
        }

        let best = s_ev.max(h_ev);
        self.player_cache.insert(key, best);
        best
    }

    // ── Initial hand EV ──

    fn can_double(&self, hand_value: u8, is_soft: bool, is_split: bool) -> bool {
        if is_split && !self.rules.double_after_split {
            return false;
        }
        match self.rules.double_restriction {
            DoubleRestriction::Any => true,
            DoubleRestriction::Hard9_10_11 => !is_soft && (9..=11).contains(&hand_value),
            DoubleRestriction::Hard10_11 => !is_soft && (10..=11).contains(&hand_value),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn initial_hand_ev(
        &mut self,
        shoe: Shoe,
        p1_idx: usize,
        p2_idx: usize,
        p_value: u8,
        p_soft: bool,
        d_idx: usize,
        split_depth: u8,
        can_surrender: bool,
    ) -> f64 {
        let dp = self.dealer_probs_from_upcard(shoe, d_idx as u8);

        // Stand
        let s_ev = Self::stand_ev(p_value, &dp);

        // Hit
        let total = Self::shoe_total(&shoe);
        let mut h_ev = 0.0;
        if total > 0 {
            for i in 0..10 {
                if shoe[i] == 0 {
                    continue;
                }
                let p = shoe[i] as f64 / total as f64;
                let cv = Self::card_value(i);
                let (nv, ns) = Self::add_to_hand(p_value, p_soft, cv);
                let new_shoe = Self::remove_card(&shoe, i);
                if nv > 21 {
                    h_ev += -p;
                } else {
                    h_ev += p * self.hit_or_stand_ev(new_shoe, nv, ns, d_idx as u8);
                }
            }
        }

        // Double
        let dbl_ev = if self.can_double(p_value, p_soft, split_depth > 0) && total > 0 {
            let mut ev = 0.0;
            for i in 0..10 {
                if shoe[i] == 0 {
                    continue;
                }
                let p = shoe[i] as f64 / total as f64;
                let cv = Self::card_value(i);
                let (nv, _ns) = Self::add_to_hand(p_value, p_soft, cv);
                let new_shoe = Self::remove_card(&shoe, i);
                if nv > 21 {
                    ev += -p;
                } else {
                    let dp2 = self.dealer_probs_from_upcard(new_shoe, d_idx as u8);
                    ev += p * Self::stand_ev(nv, &dp2);
                }
            }
            2.0 * ev
        } else {
            f64::NEG_INFINITY
        };

        // Surrender
        let sur_ev = if can_surrender && split_depth == 0 {
            -0.5
        } else {
            f64::NEG_INFINITY
        };

        let best_no_split = s_ev.max(h_ev).max(dbl_ev).max(sur_ev);

        // Split
        if p1_idx == p2_idx && split_depth < self.rules.max_splits {
            let sp_ev = self.split_ev(shoe, p1_idx, d_idx, split_depth);
            if p1_idx == 9 {
                // 10-value cards split by rank only.
                let n = self.rules.num_decks as f64;
                let per_rank = 4.0 * n;
                let total_10 = 16.0 * n;
                let same_rank_frac =
                    4.0 * (per_rank * (per_rank - 1.0)) / (total_10 * (total_10 - 1.0));
                same_rank_frac * best_no_split.max(sp_ev) + (1.0 - same_rank_frac) * best_no_split
            } else {
                best_no_split.max(sp_ev)
            }
        } else {
            best_no_split
        }
    }

    // ── Split EV (approximation) ──

    fn split_ev(&mut self, shoe: Shoe, pair_idx: usize, d_idx: usize, split_depth: u8) -> f64 {
        let is_ace_split = pair_idx == 0;
        let card_val = Self::card_value(pair_idx);
        let (base_val, base_soft) = Self::add_to_hand(0, false, card_val);

        let total = Self::shoe_total(&shoe);
        if total == 0 {
            return 0.0;
        }

        let mut one_hand_ev = 0.0;

        for i in 0..10 {
            if shoe[i] == 0 {
                continue;
            }
            let p = shoe[i] as f64 / total as f64;
            let cv = Self::card_value(i);
            let (hand_val, hand_soft) = Self::add_to_hand(base_val, base_soft, cv);
            let new_shoe = Self::remove_card(&shoe, i);

            if is_ace_split {
                if i == 0 && self.rules.resplit_aces && split_depth + 1 < self.rules.max_splits {
                    // Drew another ace — resplit
                    one_hand_ev += p * self.split_ev(new_shoe, 0, d_idx, split_depth + 1);
                } else {
                    // Split aces: one card only, must stand. Not blackjack even if 21.
                    let dp = self.dealer_probs_from_upcard(new_shoe, d_idx as u8);
                    one_hand_ev += p * Self::stand_ev(hand_val, &dp);
                }
            } else {
                one_hand_ev += p * self.initial_hand_ev(
                    new_shoe,
                    pair_idx,
                    i,
                    hand_val,
                    hand_soft,
                    d_idx,
                    split_depth + 1,
                    false,
                );
            }
        }

        2.0 * one_hand_ev
    }

    // ── Aggregate ──

    fn aggregate_ev(&mut self) -> f64 {
        let shoe = self.shoe;
        let total = self.total_cards;
        let mut ev_sum = 0.0;

        let bj_payout = self.rules.blackjack_payout.numerator as f64
            / self.rules.blackjack_payout.denominator as f64;

        for p1 in 0..10 {
            if shoe[p1] == 0 {
                continue;
            }
            let shoe1 = Self::remove_card(&shoe, p1);
            let total1 = total - 1;

            for p2 in 0..10 {
                if shoe1[p2] == 0 {
                    continue;
                }
                let shoe2 = Self::remove_card(&shoe1, p2);
                let total2 = total1 - 1;

                let cv1 = Self::card_value(p1);
                let cv2 = Self::card_value(p2);
                let (v1, s1) = Self::add_to_hand(0, false, cv1);
                let (p_val, p_soft) = Self::add_to_hand(v1, s1, cv2);
                let player_bj = p_val == 21;

                for d in 0..10 {
                    if shoe2[d] == 0 {
                        continue;
                    }
                    let shoe3 = Self::remove_card(&shoe2, d);

                    let prob = (shoe[p1] as f64 / total as f64)
                        * (shoe1[p2] as f64 / total1 as f64)
                        * (shoe2[d] as f64 / total2 as f64);

                    let d_val_raw = Self::card_value(d);
                    let dealer_can_bj = d_val_raw == 1 || d_val_raw == 10;

                    if player_bj {
                        if dealer_can_bj {
                            let remaining = Self::shoe_total(&shoe3) as f64;
                            let p_dealer_bj = if remaining == 0.0 {
                                0.0
                            } else if d_val_raw == 1 {
                                shoe3[9] as f64 / remaining
                            } else {
                                shoe3[0] as f64 / remaining
                            };
                            ev_sum += prob * (1.0 - p_dealer_bj) * bj_payout;
                        } else {
                            ev_sum += prob * bj_payout;
                        }
                    } else if self.rules.dealer_peeks && dealer_can_bj {
                        // Peek game with BJ-possible upcard.
                        let remaining = Self::shoe_total(&shoe3) as f64;
                        let p_dealer_bj = if remaining == 0.0 {
                            0.0
                        } else if d_val_raw == 1 {
                            shoe3[9] as f64 / remaining
                        } else {
                            shoe3[0] as f64 / remaining
                        };
                        if self.rules.allow_surrender && !self.rules.late_surrender {
                            // Early surrender: player can surrender before dealer peeks.
                            let normal_ev =
                                self.initial_hand_ev(shoe3, p1, p2, p_val, p_soft, d, 0, false);
                            let peek_ev = -p_dealer_bj + (1.0 - p_dealer_bj) * normal_ev;
                            ev_sum += prob * peek_ev.max(-0.5);
                        } else {
                            // Late surrender (in no-BJ branch) or no surrender.
                            let can_sur = self.rules.allow_surrender && self.rules.late_surrender;
                            let normal_ev =
                                self.initial_hand_ev(shoe3, p1, p2, p_val, p_soft, d, 0, can_sur);
                            ev_sum += prob * (-p_dealer_bj + (1.0 - p_dealer_bj) * normal_ev);
                        }
                    } else {
                        // No peek, or upcard can't make BJ.
                        let normal_ev = self.initial_hand_ev(
                            shoe3,
                            p1,
                            p2,
                            p_val,
                            p_soft,
                            d,
                            0,
                            self.rules.allow_surrender,
                        );
                        ev_sum += prob * normal_ev;
                    }
                }
            }
        }

        ev_sum
    }
}

#[cfg(test)]
mod tests;
