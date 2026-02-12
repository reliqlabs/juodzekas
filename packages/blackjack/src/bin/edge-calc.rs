use blackjack::{DoubleRestriction, EdgeCalculator, GameRules, PayoutRatio};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "edge-calc",
    about = "Calculate blackjack house edge for a rule configuration"
)]
struct Args {
    /// Use a preset: default, european, atlantic_city, single_deck
    #[arg(long)]
    preset: Option<String>,

    /// Number of decks (only 1-2 practical)
    #[arg(long, default_value = "1")]
    num_decks: u8,

    /// Dealer hits soft 17
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    dealer_hits_soft_17: bool,

    /// Dealer peeks for blackjack
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    dealer_peeks: bool,

    /// Allow surrender
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    allow_surrender: bool,

    /// Double after split allowed
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    double_after_split: bool,

    /// Double restriction: any, hard9_10_11, hard10_11
    #[arg(long, default_value = "any")]
    double_restriction: String,

    /// Maximum splits per hand (0 = no splitting)
    #[arg(long, default_value = "1")]
    max_splits: u8,

    /// Blackjack payout ratio (e.g. "3:2", "6:5", "1:1")
    #[arg(long, default_value = "3:2")]
    blackjack_payout: String,
}

fn main() {
    let args = Args::parse();

    let rules = if let Some(preset) = &args.preset {
        match preset.as_str() {
            "default" => GameRules::default(),
            "european" => GameRules::european(),
            "atlantic_city" => GameRules::atlantic_city(),
            "single_deck" => GameRules::single_deck(),
            _ => {
                eprintln!(
                    "Unknown preset '{preset}'. Available: default, european, atlantic_city, single_deck"
                );
                std::process::exit(1);
            }
        }
    } else {
        let payout = parse_payout(&args.blackjack_payout);
        let double_restriction = parse_double_restriction(&args.double_restriction);

        GameRules {
            num_decks: args.num_decks,
            dealer_hits_soft_17: args.dealer_hits_soft_17,
            allow_surrender: args.allow_surrender,
            late_surrender: args.allow_surrender,
            double_after_split: args.double_after_split,
            double_restriction,
            allow_resplit: args.max_splits > 1,
            max_splits: args.max_splits,
            resplit_aces: false,
            dealer_peeks: args.dealer_peeks,
            blackjack_payout: payout,
        }
    };

    if rules.num_decks > 2 {
        eprintln!(
            "Warning: {}-deck calculation may be very slow (combinatorial explosion)",
            rules.num_decks
        );
    }

    eprintln!("Configuration:");
    eprintln!("  Decks:              {}", rules.num_decks);
    eprintln!(
        "  Dealer soft 17:     {}",
        if rules.dealer_hits_soft_17 {
            "hits"
        } else {
            "stands"
        }
    );
    eprintln!("  Dealer peeks:       {}", rules.dealer_peeks);
    eprintln!("  Surrender:          {}", rules.allow_surrender);
    eprintln!("  Double after split: {}", rules.double_after_split);
    eprintln!("  Double restriction: {:?}", rules.double_restriction);
    eprintln!("  Max splits:         {}", rules.max_splits);
    eprintln!(
        "  BJ payout:          {}:{}",
        rules.blackjack_payout.numerator, rules.blackjack_payout.denominator
    );
    eprintln!("Calculating...");

    let result = EdgeCalculator::new(rules).calculate();

    println!("House edge:     {:+.4}%", result.house_edge * 100.0);
    println!("Player return:  {:+.4}%", result.expected_return * 100.0);

    if result.house_edge > 0.0 {
        println!("Result: House advantage");
    } else {
        println!("Result: Player advantage (negative edge)");
    }
}

fn parse_payout(s: &str) -> PayoutRatio {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        eprintln!("Invalid payout ratio '{s}', expected N:D (e.g. 3:2)");
        std::process::exit(1);
    }
    let num: u16 = parts[0].parse().unwrap_or_else(|_| {
        eprintln!("Invalid numerator in payout ratio '{s}'");
        std::process::exit(1);
    });
    let den: u16 = parts[1].parse().unwrap_or_else(|_| {
        eprintln!("Invalid denominator in payout ratio '{s}'");
        std::process::exit(1);
    });
    PayoutRatio::new(num, den).unwrap_or_else(|e| {
        eprintln!("Invalid payout ratio: {e}");
        std::process::exit(1);
    })
}

fn parse_double_restriction(s: &str) -> DoubleRestriction {
    match s.to_lowercase().as_str() {
        "any" => DoubleRestriction::Any,
        "hard9_10_11" => DoubleRestriction::Hard9_10_11,
        "hard10_11" => DoubleRestriction::Hard10_11,
        _ => {
            eprintln!("Invalid double restriction '{s}'. Options: any, hard9_10_11, hard10_11");
            std::process::exit(1);
        }
    }
}
