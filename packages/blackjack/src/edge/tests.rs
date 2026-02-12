use super::*;
use crate::PayoutRatio;

fn standard_single_deck() -> GameRules {
    GameRules {
        num_decks: 1,
        dealer_hits_soft_17: false,
        allow_surrender: false,
        late_surrender: false,
        double_after_split: true,
        double_restriction: DoubleRestriction::Any,
        allow_resplit: false,
        max_splits: 1,
        resplit_aces: false,
        dealer_peeks: true,
        blackjack_payout: PayoutRatio::THREE_TO_TWO,
    }
}

#[test]
fn test_shoe_initial() {
    let shoe = EdgeCalculator::initial_shoe(1);
    assert_eq!(shoe, [4, 4, 4, 4, 4, 4, 4, 4, 4, 16]);
    assert_eq!(EdgeCalculator::shoe_total(&shoe), 52);
}

#[test]
fn test_add_to_hand_basic() {
    assert_eq!(EdgeCalculator::add_to_hand(5, false, 3), (8, false));
    assert_eq!(EdgeCalculator::add_to_hand(0, false, 1), (11, true));
    assert_eq!(EdgeCalculator::add_to_hand(15, true, 8), (13, false));
    assert_eq!(EdgeCalculator::add_to_hand(10, false, 1), (21, true));
    assert_eq!(EdgeCalculator::add_to_hand(11, true, 10), (21, true));
    assert_eq!(EdgeCalculator::add_to_hand(15, false, 1), (16, false));
}

#[test]
fn test_dealer_probs_hard_17_stands() {
    let mut calc = EdgeCalculator::new(standard_single_deck());
    let shoe = EdgeCalculator::initial_shoe(1);
    let probs = calc.dealer_probs(shoe, 17, false);
    assert!((probs[1] - 1.0).abs() < 1e-10);
}

#[test]
fn test_dealer_probs_soft_17_h17() {
    let mut rules = standard_single_deck();
    rules.dealer_hits_soft_17 = true;
    let mut calc = EdgeCalculator::new(rules);
    let shoe = EdgeCalculator::initial_shoe(1);
    let probs = calc.dealer_probs(shoe, 17, true);
    assert!(probs[1] < 1.0);
    let sum: f64 = probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-10);
}

#[test]
fn test_dealer_probs_sum_to_one() {
    let mut calc = EdgeCalculator::new(standard_single_deck());
    let shoe = EdgeCalculator::initial_shoe(1);
    let probs = calc.dealer_probs(shoe, 6, false);
    let sum: f64 = probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-9, "Sum = {}", sum);
}

#[test]
fn test_conditioned_dealer_probs_no_bj() {
    let mut calc = EdgeCalculator::new(standard_single_deck());
    let shoe = EdgeCalculator::initial_shoe(1);
    // Ace upcard with peek: hole card can't be 10-value
    let probs = calc.dealer_probs_from_upcard(shoe, 0);
    let sum: f64 = probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-9, "Sum = {}", sum);
    // P(21) should be lower than unconditioned since BJ excluded
    let uncond = calc.dealer_probs(shoe, 11, true);
    assert!(probs[5] < uncond[5], "Conditioned P(21) should be less");
}

#[test]
fn test_stand_ev_20_vs_bust() {
    let dp = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    assert!((EdgeCalculator::stand_ev(20, &dp) - 1.0).abs() < 1e-10);
}

#[test]
fn test_stand_ev_push() {
    let dp = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    assert!((EdgeCalculator::stand_ev(20, &dp) - 0.0).abs() < 1e-10);
}

#[test]
fn test_single_deck_s17_3to2_edge() {
    let mut calc = EdgeCalculator::new(standard_single_deck());
    let result = calc.calculate();
    // Composition-dependent optimal play, single deck S17 3:2 DAS peek.
    // Expected: slightly player-favorable, within ~0.5% of zero.
    assert!(
        result.house_edge.abs() < 0.005,
        "House edge {:.4}% out of range",
        result.house_edge * 100.0
    );
}

#[test]
fn test_6to5_increases_edge() {
    let mut rules = standard_single_deck();
    rules.blackjack_payout = PayoutRatio::SIX_TO_FIVE;
    let result_65 = EdgeCalculator::new(rules).calculate();
    let result_32 = EdgeCalculator::new(standard_single_deck()).calculate();
    let diff = result_65.house_edge - result_32.house_edge;
    assert!(
        diff > 0.01,
        "6:5 should add >1% edge, got {:.4}%",
        diff * 100.0
    );
}

#[test]
fn test_h17_increases_edge() {
    let result_s17 = EdgeCalculator::new(standard_single_deck()).calculate();
    let mut rules_h17 = standard_single_deck();
    rules_h17.dealer_hits_soft_17 = true;
    let result_h17 = EdgeCalculator::new(rules_h17).calculate();
    assert!(
        result_h17.house_edge > result_s17.house_edge,
        "H17 ({:.4}%) should > S17 ({:.4}%)",
        result_h17.house_edge * 100.0,
        result_s17.house_edge * 100.0
    );
}

#[test]
fn test_no_split_rules() {
    let mut rules = standard_single_deck();
    rules.max_splits = 0;
    let result = EdgeCalculator::new(rules).calculate();
    assert!(result.house_edge.is_finite());
}

#[test]
fn test_surrender_lowers_edge() {
    let result_no = EdgeCalculator::new(standard_single_deck()).calculate();
    let mut rules_sur = standard_single_deck();
    rules_sur.allow_surrender = true;
    let result_sur = EdgeCalculator::new(rules_sur).calculate();
    assert!(
        result_sur.house_edge <= result_no.house_edge,
        "Surrender ({:.4}%) should <= no surrender ({:.4}%)",
        result_sur.house_edge * 100.0,
        result_no.house_edge * 100.0
    );
}

#[test]
fn test_early_surrender_better_than_late() {
    let mut early = standard_single_deck();
    early.allow_surrender = true;
    early.late_surrender = false;
    let result_early = EdgeCalculator::new(early).calculate();

    let mut late = standard_single_deck();
    late.allow_surrender = true;
    late.late_surrender = true;
    let result_late = EdgeCalculator::new(late).calculate();

    // Early surrender is more player-favorable (lower house edge).
    assert!(
        result_early.house_edge < result_late.house_edge,
        "Early ({:.4}%) should < late ({:.4}%)",
        result_early.house_edge * 100.0,
        result_late.house_edge * 100.0
    );
}

#[test]
fn test_resplit_lowers_edge() {
    let mut no_resplit = standard_single_deck();
    no_resplit.max_splits = 1;
    let result_no = EdgeCalculator::new(no_resplit).calculate();

    let mut with_resplit = standard_single_deck();
    with_resplit.max_splits = 3;
    with_resplit.allow_resplit = true;
    let result_resplit = EdgeCalculator::new(with_resplit).calculate();

    // More splits = more player-favorable.
    assert!(
        result_resplit.house_edge <= result_no.house_edge,
        "Resplit ({:.4}%) should <= no resplit ({:.4}%)",
        result_resplit.house_edge * 100.0,
        result_no.house_edge * 100.0
    );
}

#[test]
fn test_resplit_aces() {
    let mut no_resplit_aces = standard_single_deck();
    no_resplit_aces.max_splits = 3;
    no_resplit_aces.allow_resplit = true;
    no_resplit_aces.resplit_aces = false;
    let result_no = EdgeCalculator::new(no_resplit_aces).calculate();

    let mut with_resplit_aces = standard_single_deck();
    with_resplit_aces.max_splits = 3;
    with_resplit_aces.allow_resplit = true;
    with_resplit_aces.resplit_aces = true;
    let result_yes = EdgeCalculator::new(with_resplit_aces).calculate();

    // Resplitting aces is player-favorable.
    assert!(
        result_yes.house_edge <= result_no.house_edge,
        "Resplit aces ({:.4}%) should <= no resplit aces ({:.4}%)",
        result_yes.house_edge * 100.0,
        result_no.house_edge * 100.0
    );
}
