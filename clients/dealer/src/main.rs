use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{BigInteger, PrimeField};
use ark_std::UniformRand;
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, Subcommand};
use mob::{ChainConfig, Client, RustSigner};
use prost::Message;
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};
use std::sync::Arc;
use zk_shuffle::babyjubjub::{Fr, Point};
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::elgamal::{encrypt, Ciphertext, KeyPair};
use zk_shuffle::proof::{
    generate_reveal_proof_rapidsnark, generate_shuffle_proof_rapidsnark, CanonicalDeserialize,
    CanonicalSerialize,
};
use zk_shuffle::shuffle::shuffle;

// Re-export contract types
use juodzekas::msg::{
    Config as ContractConfig, DealerBalanceResponse, DoubleRestriction, GameListItem, GameResponse,
    InstantiateMsg, PayoutRatio,
};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Parser)]
#[command(name = "juodzekas-dealer", about = "Juodzekas blackjack dealer daemon")]
struct Cli {
    /// Dealer mnemonic
    #[arg(long, env = "DEALER_MNEMONIC")]
    mnemonic: String,

    #[arg(
        long,
        env = "RPC_URL",
        default_value = "https://rpc.xion-testnet-2.burnt.com:443"
    )]
    rpc_url: String,

    #[arg(long, env = "CHAIN_ID", default_value = "xion-testnet-2")]
    chain_id: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Instantiate contract from an already-uploaded code ID and deposit bankroll
    Init {
        /// Code ID from a previous wasm upload
        #[arg(long)]
        code_id: u64,

        /// Initial bankroll amount in base denom
        #[arg(long)]
        bankroll: u128,

        /// Token denomination
        #[arg(long, default_value = "uxion")]
        denom: String,

        /// Minimum bet
        #[arg(long, default_value = "100000")]
        min_bet: u128,

        /// Maximum bet
        #[arg(long, default_value = "1000000")]
        max_bet: u128,

        /// Blackjack payout ratio (e.g. "3:2")
        #[arg(long, default_value = "3:2")]
        blackjack_payout: String,

        /// Dealer hits soft 17
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        dealer_hits_soft_17: bool,

        /// Dealer peeks for blackjack
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        dealer_peeks: bool,

        /// Double restriction: any, hard9_10_11, hard10_11
        #[arg(long, default_value = "any")]
        double_restriction: String,

        /// Maximum number of splits
        #[arg(long, default_value = "3")]
        max_splits: u32,

        /// Allow splitting aces
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        can_split_aces: bool,

        /// Allow hitting split aces
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        can_hit_split_aces: bool,

        /// Allow surrender
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        surrender_allowed: bool,

        /// Shuffle verification key ID (registered on Xion ZK module)
        #[arg(long, env = "SHUFFLE_VK_ID")]
        shuffle_vk_id: String,

        /// Reveal verification key ID (registered on Xion ZK module)
        #[arg(long, env = "REVEAL_VK_ID")]
        reveal_vk_id: String,

        /// Timeout in seconds
        #[arg(long, default_value = "3600")]
        timeout_seconds: u64,

        /// Contract label
        #[arg(long, default_value = "juodzekas-blackjack")]
        label: String,

        /// Allow deploying a config with negative house edge (player-favorable)
        #[arg(long, env = "ALLOW_NEGATIVE_EDGE", default_value_t = false, action = clap::ArgAction::Set)]
        allow_negative_edge: bool,
    },

    /// Run the dealer daemon (poll and auto-reveal)
    Run {
        #[arg(long, env = "CONTRACT_ADDR")]
        contract_addr: String,

        /// Auto-create new games after each settles
        #[arg(long, env = "AUTO_CREATE_GAME", default_value_t = true, action = clap::ArgAction::Set)]
        auto_create_game: bool,
    },

    /// Deposit bankroll into the contract
    Deposit {
        #[arg(long, env = "CONTRACT_ADDR")]
        contract_addr: String,

        /// Amount to deposit in base denom (uxion)
        #[arg(long)]
        amount: u128,

        /// Token denomination
        #[arg(long, default_value = "uxion")]
        denom: String,
    },

    /// Withdraw all bankroll and exit
    Withdraw {
        #[arg(long, env = "CONTRACT_ADDR")]
        contract_addr: String,
    },
}

struct DealerConfig {
    contract_addr: String,
    rpc_url: String,
    auto_create_game: bool,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    let signer = RustSigner::from_mnemonic(cli.mnemonic.clone(), "xion".into(), None)
        .expect("Invalid mnemonic");
    let address = signer.address();
    log::info!("Dealer address: {address}");

    let chain_config = ChainConfig::new(
        cli.chain_id.clone(),
        cli.rpc_url.clone(),
        "xion".to_string(),
    );
    let client =
        Client::new_with_signer(chain_config, Arc::new(signer)).expect("Failed to create client");

    match cli.command {
        Command::Init {
            code_id,
            bankroll,
            denom,
            min_bet,
            max_bet,
            blackjack_payout,
            dealer_hits_soft_17,
            dealer_peeks,
            double_restriction,
            max_splits,
            can_split_aces,
            can_hit_split_aces,
            surrender_allowed,
            shuffle_vk_id,
            reveal_vk_id,
            timeout_seconds,
            label,
            allow_negative_edge,
        } => {
            if let Err(e) = cmd_init(
                &client,
                &address,
                code_id,
                bankroll,
                &denom,
                min_bet,
                max_bet,
                &blackjack_payout,
                dealer_hits_soft_17,
                dealer_peeks,
                &double_restriction,
                max_splits,
                can_split_aces,
                can_hit_split_aces,
                surrender_allowed,
                &shuffle_vk_id,
                &reveal_vk_id,
                timeout_seconds,
                &label,
                allow_negative_edge,
            ) {
                log::error!("Init failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Run {
            contract_addr,
            auto_create_game,
        } => {
            let config = DealerConfig {
                contract_addr,
                rpc_url: cli.rpc_url,
                auto_create_game,
            };

            std::fs::create_dir_all("data").ok();

            loop {
                match run_game(&client, &config, &address) {
                    Ok(()) => log::info!("Game completed"),
                    Err(e) => log::error!("Game failed: {e}"),
                }

                if !config.auto_create_game {
                    log::info!("AUTO_CREATE_GAME=false, exiting");
                    break;
                }
                log::info!("Starting next game...");
            }
        }
        Command::Deposit {
            contract_addr,
            amount,
            denom,
        } => {
            log::info!("Depositing {amount} {denom} to contract {contract_addr}...");
            let msg_json = serde_json::json!({ "deposit_bankroll": {} });
            let msg_bytes = serde_json::to_vec(&msg_json).unwrap();
            let funds = vec![mob::Coin::new(&denom, amount.to_string())];
            match execute_and_confirm(&client, contract_addr, msg_bytes, funds, "Deposit bankroll")
            {
                Ok(resp) => log::info!("Deposit confirmed! Hash: {}", resp.txhash),
                Err(e) => {
                    log::error!("Deposit failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Withdraw { contract_addr } => {
            let config = DealerConfig {
                contract_addr,
                rpc_url: cli.rpc_url,
                auto_create_game: false,
            };
            match withdraw_all_bankroll(&client, &config) {
                Ok(()) => log::info!("Bankroll withdrawn successfully"),
                Err(e) => {
                    log::error!("Withdraw failed: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

// ── Init subcommand ──

#[allow(clippy::too_many_arguments)]
fn cmd_init(
    client: &Client,
    sender: &str,
    code_id: u64,
    bankroll: u128,
    denom: &str,
    min_bet: u128,
    max_bet: u128,
    blackjack_payout: &str,
    dealer_hits_soft_17: bool,
    dealer_peeks: bool,
    double_restriction: &str,
    max_splits: u32,
    can_split_aces: bool,
    can_hit_split_aces: bool,
    surrender_allowed: bool,
    shuffle_vk_id: &str,
    reveal_vk_id: &str,
    timeout_seconds: u64,
    label: &str,
    allow_negative_edge: bool,
) -> Result<(), BoxErr> {
    // Build InstantiateMsg
    let bj_payout = parse_payout_ratio(blackjack_payout)?;
    let double_res = parse_double_restriction(double_restriction)?;

    // Edge check: compute house edge for this configuration
    {
        let edge_double_restriction = match &double_res {
            DoubleRestriction::Any => blackjack::DoubleRestriction::Any,
            DoubleRestriction::Hard9_10_11 => blackjack::DoubleRestriction::Hard9_10_11,
            DoubleRestriction::Hard10_11 => blackjack::DoubleRestriction::Hard10_11,
        };
        let edge_payout = blackjack::PayoutRatio::new(bj_payout.numerator, bj_payout.denominator)
            .map_err(|e| format!("Invalid payout ratio: {e}"))?;

        let rules = blackjack::GameRules {
            num_decks: 1, // Always single deck (ZK shuffle architecture)
            dealer_hits_soft_17,
            allow_surrender: surrender_allowed,
            late_surrender: surrender_allowed,
            double_after_split: true,
            double_restriction: edge_double_restriction,
            allow_resplit: max_splits > 1,
            max_splits: max_splits as u8,
            resplit_aces: can_split_aces && can_hit_split_aces,
            dealer_peeks,
            blackjack_payout: edge_payout,
        };

        log::info!("Computing house edge for this configuration...");
        let result = blackjack::EdgeCalculator::new(rules).calculate();
        log::info!(
            "House edge: {:+.4}% (player return: {:+.4}%)",
            result.house_edge * 100.0,
            result.expected_return * 100.0
        );

        if result.house_edge < 0.0 {
            if allow_negative_edge {
                log::warn!(
                    "Negative house edge ({:+.4}%) — dealer will lose money on average. Proceeding (--allow-negative-edge set).",
                    result.house_edge * 100.0
                );
            } else {
                return Err(format!(
                    "Negative house edge ({:+.4}%). This config favors the player and will lose the dealer money. \
                     Use --allow-negative-edge or ALLOW_NEGATIVE_EDGE=true to override.",
                    result.house_edge * 100.0
                ).into());
            }
        }
    }

    let instantiate_msg = InstantiateMsg {
        denom: denom.to_string(),
        min_bet: cosmwasm_std::Uint128::new(min_bet),
        max_bet: cosmwasm_std::Uint128::new(max_bet),
        blackjack_payout: bj_payout,
        insurance_payout: PayoutRatio {
            numerator: 2,
            denominator: 1,
        },
        standard_payout: PayoutRatio {
            numerator: 1,
            denominator: 1,
        },
        dealer_hits_soft_17,
        dealer_peeks,
        double_restriction: double_res,
        max_splits,
        can_split_aces,
        can_hit_split_aces,
        surrender_allowed,
        shuffle_vk_id: shuffle_vk_id.to_string(),
        reveal_vk_id: reveal_vk_id.to_string(),
        timeout_seconds: Some(timeout_seconds),
    };
    let msg_bytes = serde_json::to_vec(&instantiate_msg)?;

    // 4. Instantiate with bankroll funds
    let funds = if bankroll > 0 {
        vec![mob::Coin::new(denom, bankroll.to_string())]
    } else {
        vec![]
    };

    log::info!("Instantiating contract (code_id={code_id}, bankroll={bankroll} {denom})...");
    let inst_response =
        broadcast_and_confirm_instantiate(client, sender, code_id, label, msg_bytes, funds)?;
    let contract_addr = extract_contract_address(&inst_response)?;
    log::info!("Contract instantiated: tx={}", inst_response.txhash);

    // 5. Print for .env
    println!("\nCONTRACT_ADDR={contract_addr}");
    log::info!("Done. Add CONTRACT_ADDR to your .env file.");

    Ok(())
}

fn broadcast_and_confirm_instantiate(
    client: &Client,
    sender: &str,
    code_id: u64,
    label: &str,
    msg_bytes: Vec<u8>,
    funds: Vec<mob::Coin>,
) -> Result<mob::TxResponse, BoxErr> {
    let broadcast = client.instantiate_contract(
        Some(sender.to_string()),
        code_id,
        Some(label.to_string()),
        msg_bytes,
        funds,
        Some("Instantiate contract".to_string()),
        None,
    )?;
    if broadcast.code != 0 {
        return Err(format!("Instantiate broadcast rejected: {}", broadcast.raw_log).into());
    }
    log::info!("Instantiate TX broadcast: {}", broadcast.txhash);
    poll_tx(client, &broadcast.txhash)
}

fn poll_tx(client: &Client, txhash: &str) -> Result<mob::TxResponse, BoxErr> {
    for attempt in 0..15 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        match client.get_tx(txhash.to_string()) {
            Ok(tx) => {
                if tx.code != 0 {
                    return Err(format!("TX failed (code {}): {}", tx.code, tx.raw_log).into());
                }
                return Ok(tx);
            }
            Err(_) if attempt < 14 => continue,
            Err(e) => return Err(format!("TX not found after polling: {e}").into()),
        }
    }
    Err("TX not confirmed after 30s".into())
}

fn extract_contract_address(tx: &mob::TxResponse) -> Result<String, BoxErr> {
    let events: serde_json::Value = serde_json::from_str(&tx.raw_log)
        .map_err(|e| format!("Failed to parse raw_log: {e}\nraw_log: {}", tx.raw_log))?;

    if let Some(arr) = events.as_array() {
        for event in arr {
            if let Some(ev_type) = event.get("type").and_then(|t| t.as_str()) {
                if ev_type == "instantiate" {
                    if let Some(attrs) = event.get("attributes").and_then(|a| a.as_array()) {
                        for attr in attrs {
                            let key = attr.get("key").and_then(|k| k.as_str()).unwrap_or("");
                            if key == "_contract_address" {
                                let val = attr.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                return Ok(val.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Err(format!(
        "contract address not found in TX events. raw_log: {}",
        tx.raw_log
    )
    .into())
}

fn parse_payout_ratio(s: &str) -> Result<PayoutRatio, BoxErr> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid payout ratio '{s}', expected format 'N:D'").into());
    }
    Ok(PayoutRatio {
        numerator: parts[0].parse()?,
        denominator: parts[1].parse()?,
    })
}

fn parse_double_restriction(s: &str) -> Result<DoubleRestriction, BoxErr> {
    match s.to_lowercase().as_str() {
        "any" => Ok(DoubleRestriction::Any),
        "hard9_10_11" => Ok(DoubleRestriction::Hard9_10_11),
        "hard10_11" => Ok(DoubleRestriction::Hard10_11),
        _ => Err(format!(
            "Invalid double restriction '{s}', expected: any, hard9_10_11, hard10_11"
        )
        .into()),
    }
}

// ── Run subcommand helpers ──

fn run_game(client: &Client, config: &DealerConfig, address: &str) -> Result<(), BoxErr> {
    let (sk, pk, game_id) = create_game(client, config, address)?;

    let key_path = format!("data/game_{game_id}_keys.bin");
    save_keys(&key_path, &sk, &pk)?;
    log::info!("Keys saved to {key_path}");

    let result = game_loop(client, config, game_id, &sk, &pk);

    // Clean up key file after game completes (settled or timed out)
    if std::fs::remove_file(&key_path).is_ok() {
        log::debug!("Cleaned up {key_path}");
    }

    result
}

fn create_game(
    client: &Client,
    config: &DealerConfig,
    address: &str,
) -> Result<(Fr, Point, u64), BoxErr> {
    // Build a local tokio runtime for proof generation (WASM calculator needs reactor)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    log::info!("Generating dealer keypair and shuffling deck...");
    let mut rng = ChaCha8Rng::from_entropy();
    let dealer_keys = KeyPair::generate(&mut rng);

    let g = Point::generator();
    let mut encrypted_deck = Vec::new();
    for i in 1..=52u64 {
        let card_point = (g.into_group() * Fr::from(i)).into_affine();
        let r = Fr::rand(&mut rng);
        let ct = encrypt(&dealer_keys.pk, &card_point, &r);
        encrypted_deck.push(ct);
    }

    log::info!("Shuffling deck...");
    let dealer_shuffle = shuffle(&mut rng, &encrypted_deck, &dealer_keys.pk);

    log::info!("Generating ZK shuffle proof (this may take ~1 minute)...");
    let dealer_proof = generate_shuffle_proof_rapidsnark(
        &dealer_shuffle.public_inputs,
        dealer_shuffle.private_inputs,
    )
    .map_err(|e| -> BoxErr { e.to_string().into() })?;
    log::info!("Proof generated");

    let proof_json = serde_json::to_string(&dealer_proof)?;
    let public_inputs_strs: Vec<String> = dealer_shuffle
        .public_inputs
        .to_ark_public_inputs()
        .iter()
        .map(|f| {
            let bigint = num_bigint::BigInt::from_bytes_le(
                num_bigint::Sign::Plus,
                &f.into_bigint().to_bytes_le(),
            );
            bigint.to_string()
        })
        .collect();

    let shuffled_deck: Vec<String> = dealer_shuffle
        .deck
        .iter()
        .map(|ct| Ok(general_purpose::STANDARD.encode(serialize_ciphertext(ct)?)))
        .collect::<Result<Vec<_>, BoxErr>>()?;

    let msg_json = serde_json::json!({
        "create_game": {
            "public_key": general_purpose::STANDARD.encode(serialize_point(&dealer_keys.pk)?),
            "shuffled_deck": shuffled_deck,
            "proof": general_purpose::STANDARD.encode(&proof_json),
            "public_inputs": public_inputs_strs,
        }
    });
    let msg_bytes = serde_json::to_vec(&msg_json)?;

    log::info!("Submitting CreateGame (using pre-deposited bankroll)...");

    // Drop the runtime guard before mob calls (mob creates its own runtime)
    drop(_rt_guard);
    drop(rt);

    let tx_response = execute_and_confirm(
        client,
        config.contract_addr.clone(),
        msg_bytes,
        vec![],
        "Create blackjack game",
    )?;

    if tx_response.code != 0 {
        return Err(format!("TX failed: {}", tx_response.raw_log).into());
    }
    log::info!("TX confirmed: {}", tx_response.txhash);

    // Find our game_id
    let rt2 = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let games: Vec<GameListItem> = rt2.block_on(query_list_games(
        &config.rpc_url,
        &config.contract_addr,
        Some("WaitingForPlayerJoin".into()),
    ))?;

    let game_id = games
        .iter()
        .find(|g| g.dealer == address)
        .map(|g| g.game_id)
        .ok_or("Could not find newly created game")?;

    log::info!("Game created: id={game_id}");
    Ok((dealer_keys.sk, dealer_keys.pk, game_id))
}

fn game_loop(
    client: &Client,
    config: &DealerConfig,
    game_id: u64,
    sk: &Fr,
    pk: &Point,
) -> Result<(), BoxErr> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // Query contract config once for timeout_seconds
    let contract_config: ContractConfig =
        rt.block_on(query_config(&config.rpc_url, &config.contract_addr))?;
    // Allow generous buffer (2x contract timeout) before dealer claims
    let claim_after = std::time::Duration::from_secs(contract_config.timeout_seconds * 2);
    let game_start = std::time::Instant::now();
    let mut consecutive_query_failures: u32 = 0;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));

        let game: GameResponse = match rt.block_on(query_game_by_id(
            &config.rpc_url,
            &config.contract_addr,
            game_id,
        )) {
            Ok(g) => {
                consecutive_query_failures = 0;
                g
            }
            Err(e) => {
                consecutive_query_failures += 1;
                log::warn!("Query failed ({consecutive_query_failures}x): {e}");
                if consecutive_query_failures >= 30 {
                    return Err(format!(
                        "Game {game_id}: giving up after {consecutive_query_failures} consecutive query failures"
                    ).into());
                }
                continue;
            }
        };

        let status = &game.status;

        if status.contains("WaitingForPlayerJoin") {
            // Check if we've waited too long for a player
            if game_start.elapsed() > claim_after {
                log::warn!(
                    "No player joined after {:?}, claiming timeout",
                    game_start.elapsed()
                );
                match claim_timeout(client, config, game_id) {
                    Ok(_) => log::info!("Timeout claimed for game {game_id}"),
                    Err(e) => log::error!("Failed to claim timeout: {e}"),
                }
                return Ok(());
            }
            log::debug!("Waiting for player to join...");
            continue;
        }

        if status.contains("WaitingForReveal") {
            handle_reveals(client, config, game_id, &game, sk, pk);
            continue;
        }

        if status.contains("OfferingInsurance") {
            log::debug!("Waiting for player insurance decision...");
        } else if status.contains("PlayerTurn") {
            log::debug!("Player's turn...");
        } else if status.contains("DealerTurn") {
            log::debug!("Dealer turn (contract auto-processes)...");
        } else if status.contains("Settled") {
            log::info!("Game {game_id} settled: {status}");
            log_game_results(&game);
            return Ok(());
        } else {
            log::warn!("Unknown game status: {status}");
        }

        // For any non-settled, non-reveal status: check if player timed out
        if game_start.elapsed() > claim_after {
            log::warn!(
                "Game {game_id} stale ({:?} elapsed), attempting timeout claim",
                game_start.elapsed()
            );
            match claim_timeout(client, config, game_id) {
                Ok(_) => {
                    log::info!("Timeout claimed for game {game_id}");
                    return Ok(());
                }
                Err(e) => {
                    // Timeout claim may fail if not enough time passed on-chain yet
                    log::debug!("Timeout claim failed (may not be eligible yet): {e}");
                }
            }
        }
    }
}

fn claim_timeout(client: &Client, config: &DealerConfig, game_id: u64) -> Result<(), BoxErr> {
    let msg_json = serde_json::json!({ "claim_timeout": { "game_id": game_id } });
    let msg_bytes = serde_json::to_vec(&msg_json)?;
    let tx = execute_and_confirm(
        client,
        config.contract_addr.clone(),
        msg_bytes,
        vec![],
        "Claim timeout",
    )?;
    if tx.code != 0 {
        return Err(format!("Claim timeout TX failed: {}", tx.raw_log).into());
    }
    Ok(())
}

fn handle_reveals(
    client: &Client,
    config: &DealerConfig,
    game_id: u64,
    game: &GameResponse,
    sk: &Fr,
    pk: &Point,
) {
    let reveal_requests = parse_reveal_requests(&game.status);

    let already_submitted: Vec<u32> = game
        .pending_reveals
        .iter()
        .filter(|pr| pr.dealer_partial.is_some())
        .map(|pr| pr.card_index)
        .collect();

    for &card_idx in &reveal_requests {
        if !already_submitted.contains(&card_idx) {
            if let Err(e) = submit_reveal(client, config, game_id, card_idx, game, sk, pk) {
                log::error!("Reveal failed for card {card_idx}: {e}");
            }
        }
    }

    for pr in &game.pending_reveals {
        if pr.dealer_partial.is_none() && !reveal_requests.contains(&pr.card_index) {
            if let Err(e) = submit_reveal(client, config, game_id, pr.card_index, game, sk, pk) {
                log::error!("Reveal failed for card {}: {e}", pr.card_index);
            }
        }
    }
}

fn submit_reveal(
    client: &Client,
    config: &DealerConfig,
    game_id: u64,
    card_index: u32,
    game: &GameResponse,
    sk: &Fr,
    pk: &Point,
) -> Result<(), BoxErr> {
    if card_index as usize >= game.deck.len() {
        return Err(format!("Invalid card_index: {card_index}").into());
    }

    // Need tokio reactor for WASM proof generator
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    let card_binary = &game.deck[card_index as usize];
    let mut cursor = card_binary.as_slice();
    let c0 = Point::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("Failed to deserialize card c0: {e}"))?;
    let c1 = Point::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("Failed to deserialize card c1: {e}"))?;
    let encrypted_card = Ciphertext { c0, c1 };

    log::info!("Revealing card {card_index}...");
    let reveal = reveal_card(sk, &encrypted_card, pk);

    log::info!("Generating reveal proof for card {card_index}...");
    let reveal_proof = generate_reveal_proof_rapidsnark(&reveal.public_inputs, reveal.sk_p)
        .map_err(|e| -> BoxErr { e.to_string().into() })?;

    let mut partial_buf = Vec::new();
    reveal
        .partial_decryption
        .serialize_compressed(&mut partial_buf)
        .map_err(|e| format!("Failed to serialize partial decryption: {e}"))?;
    let proof_json = serde_json::to_string(&reveal_proof)?;
    let public_inputs_strs: Vec<String> = reveal
        .public_inputs
        .to_ark_public_inputs()
        .iter()
        .map(|f| {
            let bigint = num_bigint::BigInt::from_bytes_le(
                num_bigint::Sign::Plus,
                &f.into_bigint().to_bytes_le(),
            );
            bigint.to_string()
        })
        .collect();

    let msg_json = serde_json::json!({
        "submit_reveal": {
            "game_id": game_id,
            "card_index": card_index,
            "partial_decryption": general_purpose::STANDARD.encode(&partial_buf),
            "proof": general_purpose::STANDARD.encode(&proof_json),
            "public_inputs": public_inputs_strs,
        }
    });
    let msg_bytes = serde_json::to_vec(&msg_json)?;

    // Drop runtime before mob call
    drop(_rt_guard);
    drop(rt);

    log::info!("Submitting reveal TX for card {card_index}...");
    let tx_response = execute_and_confirm(
        client,
        config.contract_addr.clone(),
        msg_bytes,
        vec![],
        "Submit Reveal",
    )?;

    if tx_response.code != 0 {
        return Err(format!("Reveal TX failed: {}", tx_response.raw_log).into());
    }
    log::info!(
        "Reveal for card {card_index} confirmed: {}",
        tx_response.txhash
    );
    Ok(())
}

fn withdraw_all_bankroll(client: &Client, config: &DealerConfig) -> Result<(), BoxErr> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let balance: DealerBalanceResponse =
        rt.block_on(query_dealer_balance(&config.rpc_url, &config.contract_addr))?;
    drop(rt);

    if balance.balance.is_zero() {
        log::info!("Dealer balance is zero, nothing to withdraw");
        return Ok(());
    }

    log::info!("Withdrawing {} from bankroll...", balance.balance);
    let msg_json = serde_json::json!({ "withdraw_bankroll": {} });
    let msg_bytes = serde_json::to_vec(&msg_json)?;

    let tx_response = execute_and_confirm(
        client,
        config.contract_addr.clone(),
        msg_bytes,
        vec![],
        "Withdraw bankroll",
    )?;

    if tx_response.code != 0 {
        return Err(format!("Withdraw TX failed: {}", tx_response.raw_log).into());
    }
    log::info!("Withdraw confirmed: {}", tx_response.txhash);
    Ok(())
}

// ── Helpers ──

fn parse_reveal_requests(status: &str) -> Vec<u32> {
    if !status.contains("reveal_requests:") {
        return vec![];
    }
    status
        .split("reveal_requests:")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .map(|s| s.trim_start_matches([' ', '[']))
        .map(|s| {
            s.split(',')
                .filter_map(|n| n.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn log_game_results(game: &GameResponse) {
    for (i, hand) in game.hands.iter().enumerate() {
        let cards: Vec<String> = hand
            .cards
            .iter()
            .map(|&idx| blackjack::Card::from_index(idx as usize).to_display())
            .collect();
        log::info!(
            "  Hand {}: [{}] - bet: {} - status: {}",
            i,
            cards.join(", "),
            hand.bet,
            hand.status
        );
    }
    let dealer_cards: Vec<String> = game
        .dealer_hand
        .iter()
        .map(|&idx| blackjack::Card::from_index(idx as usize).to_display())
        .collect();
    log::info!("  Dealer: [{}]", dealer_cards.join(", "));
}

fn serialize_point(p: &Point) -> Result<Vec<u8>, BoxErr> {
    let mut buf = Vec::new();
    p.serialize_compressed(&mut buf)
        .map_err(|e| format!("Failed to serialize point: {e}"))?;
    Ok(buf)
}

fn serialize_ciphertext(ct: &Ciphertext) -> Result<Vec<u8>, BoxErr> {
    let mut buf = Vec::new();
    ct.c0
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("Failed to serialize ciphertext c0: {e}"))?;
    ct.c1
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("Failed to serialize ciphertext c1: {e}"))?;
    Ok(buf)
}

fn save_keys(path: &str, sk: &Fr, pk: &Point) -> Result<(), BoxErr> {
    let mut data = Vec::new();
    sk.serialize_compressed(&mut data)
        .map_err(|e| format!("Failed to serialize sk: {e}"))?;
    pk.serialize_compressed(&mut data)
        .map_err(|e| format!("Failed to serialize pk: {e}"))?;
    std::fs::write(path, &data)?;
    Ok(())
}

// ── Contract queries (async) ──

async fn query_contract_raw(
    rpc_url: &str,
    contract_addr: &str,
    query_msg: &[u8],
) -> Result<Vec<u8>, BoxErr> {
    use tendermint_rpc::{Client as TmClient, HttpClient};

    let path = "/cosmwasm.wasm.v1.Query/SmartContractState";
    let data = {
        let req = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateRequest {
            address: contract_addr.to_string(),
            query_data: query_msg.to_vec(),
        };
        req.encode_to_vec()
    };

    let tm_client = HttpClient::new(rpc_url)?;
    let response = tm_client
        .abci_query(Some(path.to_string()), data, None, false)
        .await?;

    if response.code.is_err() {
        return Err(format!("ABCI query failed: {}", response.log).into());
    }

    let res_wrapper = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateResponse::decode(
        response.value.as_slice(),
    )?;
    Ok(res_wrapper.data)
}

async fn query_game_by_id(
    rpc_url: &str,
    contract_addr: &str,
    game_id: u64,
) -> Result<GameResponse, BoxErr> {
    let query_bytes =
        serde_json::to_vec(&serde_json::json!({ "get_game": { "game_id": game_id } }))?;
    let response_bytes = query_contract_raw(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

async fn query_dealer_balance(
    rpc_url: &str,
    contract_addr: &str,
) -> Result<DealerBalanceResponse, BoxErr> {
    let query_bytes = serde_json::to_vec(&serde_json::json!({ "get_dealer_balance": {} }))?;
    let response_bytes = query_contract_raw(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

async fn query_config(rpc_url: &str, contract_addr: &str) -> Result<ContractConfig, BoxErr> {
    let query_bytes = serde_json::to_vec(&serde_json::json!({ "get_config": {} }))?;
    let response_bytes = query_contract_raw(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

async fn query_list_games(
    rpc_url: &str,
    contract_addr: &str,
    status_filter: Option<String>,
) -> Result<Vec<GameListItem>, BoxErr> {
    let query_bytes = serde_json::to_vec(
        &serde_json::json!({ "list_games": { "status_filter": status_filter } }),
    )?;
    let response_bytes = query_contract_raw(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

// ── TX execution (sync, non-tokio thread) ──

fn execute_and_confirm(
    client: &Client,
    contract_addr: String,
    msg_bytes: Vec<u8>,
    funds: Vec<mob::Coin>,
    memo: &str,
) -> Result<mob::TxResponse, BoxErr> {
    let broadcast = client.execute_contract(
        contract_addr,
        msg_bytes,
        funds,
        Some(memo.to_string()),
        None,
    )?;

    if broadcast.code != 0 {
        return Err(format!("Broadcast rejected: {}", broadcast.raw_log).into());
    }

    poll_tx(client, &broadcast.txhash)
}
