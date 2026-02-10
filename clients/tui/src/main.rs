use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{error::Error, io};
use std::sync::{Arc, Mutex};
use std::sync::mpsc as std_mpsc;
use base64::{engine::general_purpose, Engine as _};
use ark_ff::{BigInteger, PrimeField};

mod game;
use game::{GameState, GameMode};

mod game_logic;

mod contract_msg;

mod tui_logger;
use tui_logger::TuiLogger;

#[cfg(feature = "wallet")]
mod wallet;
#[cfg(feature = "wallet")]
use wallet::Wallet;

#[derive(PartialEq)]
enum GamePhase {
    ModeSelection,
    SpotSelection,
    Initializing,
    PlayerTurn,
    DealerTurn,
    GameOver,
    // Contract mode phases
    ContractSetup,        // Wallet and contract connection
    WaitingForReveal,     // Waiting for opponent to reveal card
}

#[derive(Clone, Copy, PartialEq)]
enum SpotOutcome {
    Win,
    Loss,
    Push,
    Surrender,
}

#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    Normal,
    #[cfg(feature = "wallet")]
    Mnemonic,
    #[cfg(feature = "wallet")]
    ContractAddress,
}

#[cfg(feature = "wallet")]
#[allow(dead_code)]
enum Action {
    BalanceUpdated(String),
    GamesListed(Vec<contract_msg::GameListItem>),
    GameStateUpdated(contract_msg::GameResponse),
    WalletConnected(mob::Client),
    GameJoined { client: mob::Client, sk: zk_shuffle::babyjubjub::Fr, pk: zk_shuffle::babyjubjub::Point },
    TxCompleted { action_name: String, client: mob::Client, txhash: String },
    RevealSubmitted { client: mob::Client, card_index: u32 },
    TxFailed { action_name: String, client: mob::Client, error: String },
    OpFailed { op_name: String, error: String },
}

struct App {
    input_mode: InputMode,
    game_state: Option<GameState>,
    phase: GamePhase,
    selected_mode: Option<GameMode>,
    selected_spots: Option<usize>,
    status: String,
    logs: Vec<String>,
    log_buffer: Arc<Mutex<Vec<String>>>, // Shared buffer for capturing log:: messages
    loading_dots: usize, // 0-3 for animated loading dots
    init_task: Option<tokio::task::JoinHandle<Result<GameState, String>>>,
    init_start_time: Option<std::time::Instant>,
    current_init_stage: String, // e.g., "Loading keys", "Shuffling"
    next_game_task: Option<tokio::task::JoinHandle<Result<GameState, String>>>, // Background pre-shuffle for next game
    spot_outcomes: Vec<Vec<SpotOutcome>>, // Track outcome for each hand in each spot at end of round
    log_visible: bool, // Toggle for log visibility
    help_visible: bool, // Toggle for help modal
    // Contract mode fields
    #[cfg(feature = "wallet")]
    wallet: Option<Wallet>,
    contract_address: Option<String>,
    game_id: Option<u64>, // Current game ID
    rpc_url: String,
    chain_id: String,
    contract_address_input: String, // Buffer for typing contract address
    mnemonic_input: String, // Buffer for typing mnemonic phrase
    available_games: Vec<contract_msg::GameListItem>, // List of games player can join
    contract_game_state: Option<contract_msg::GameResponse>, // Current contract game state for display
    zk_keys: Option<(zk_shuffle::babyjubjub::Fr, zk_shuffle::babyjubjub::Point)>, // (sk, pk) for contract mode reveals
    wallet_balance: Option<String>, // Wallet balance (e.g., "1000uxion")
    last_balance_poll: Option<std::time::Instant>, // Last time balance was polled
    // Non-blocking action channel
    #[cfg(feature = "wallet")]
    action_tx: std_mpsc::Sender<Action>,
    #[cfg(feature = "wallet")]
    action_rx: std_mpsc::Receiver<Action>,
    #[cfg(feature = "wallet")]
    pending_op: Option<String>,
    #[cfg(feature = "wallet")]
    pending_op_start: Option<std::time::Instant>,
    #[cfg(feature = "wallet")]
    balance_poll_inflight: bool,
    #[cfg(feature = "wallet")]
    game_poll_inflight: bool,
    last_game_poll: Option<std::time::Instant>,
}

impl App {
    fn new(log_buffer: Arc<Mutex<Vec<String>>>) -> App {
        #[cfg(feature = "wallet")]
        let (action_tx, action_rx) = std_mpsc::channel();
        App {
            input_mode: InputMode::Normal,
            game_state: None,
            phase: GamePhase::ModeSelection,
            selected_mode: None,
            selected_spots: None,
            status: "Select mode: [F]ast (instant), [T]rustless (~1 min, ZK proofs), or [C]ontract (on-chain)".to_string(),
            logs: vec![
                "Welcome to Juodžekas!".to_string(),
                "Choose your game mode:".to_string(),
                "[F] Fast - Instant gameplay, no proofs".to_string(),
                "[T] Trustless - Full ZK proofs, ~1 min setup".to_string(),
                "[C] Contract - On-chain with smart contract".to_string(),
            ],
            log_buffer,
            loading_dots: 0,
            init_task: None,
            init_start_time: None,
            current_init_stage: String::new(),
            next_game_task: None,
            spot_outcomes: Vec::new(),
            log_visible: true,
            help_visible: false,
            #[cfg(feature = "wallet")]
            wallet: None,
            contract_address: None,
            game_id: None,
            rpc_url: "https://rpc.xion-testnet-2.burnt.com:443".to_string(),
            chain_id: "xion-testnet-2".to_string(),
            contract_address_input: String::new(),
            mnemonic_input: String::new(),
            available_games: Vec::new(),
            contract_game_state: None,
            zk_keys: None,
            wallet_balance: None,
            last_balance_poll: None,
            #[cfg(feature = "wallet")]
            action_tx,
            #[cfg(feature = "wallet")]
            action_rx,
            #[cfg(feature = "wallet")]
            pending_op: None,
            #[cfg(feature = "wallet")]
            pending_op_start: None,
            #[cfg(feature = "wallet")]
            balance_poll_inflight: false,
            #[cfg(feature = "wallet")]
            game_poll_inflight: false,
            last_game_poll: None,
        }
    }

    /// Load wallet from mnemonic. Does NO network calls — just sets up wallet and state.
    /// Network operations (balance, game list) happen lazily in the event loop.
    #[cfg(feature = "wallet")]
    fn load_wallet_from_mnemonic(&mut self, mnemonic: &str) -> bool {
        match Wallet::from_mnemonic(mnemonic, "xion") {
            Ok(wallet) => {
                self.add_log(format!("Wallet loaded: {}", wallet.address()));
                self.wallet = Some(wallet);
                self.input_mode = InputMode::Normal;

                if let Ok(addr) = std::env::var("CONTRACT_ADDR") {
                    self.contract_address = Some(addr.clone());
                    self.add_log(format!("Contract: {addr}"));
                    self.add_log("Press [L] to list games, or [J] to join after selecting".to_string());
                    self.status = "Press [L] to list available games".to_string();
                } else {
                    self.add_log("Enter contract address".to_string());
                    self.status = "Enter contract address".to_string();
                    self.input_mode = InputMode::ContractAddress;
                }
                true
            }
            Err(e) => {
                self.add_log(format!("Failed to load wallet: {e}"));
                false
            }
        }
    }

    /// Convert atomic denom units to human-readable format.
    /// E.g. 1_000_000 uxion -> "1", 10_500_000 uxion -> "10.5"
    fn format_denom(amount: u128, denom: &str) -> String {
        let display_denom = if denom.starts_with('u') { &denom[1..] } else { denom };
        let decimals: u32 = if denom.starts_with('u') { 6 } else { 0 };
        if decimals == 0 {
            return format!("{amount} {display_denom}");
        }
        let factor = 10u128.pow(decimals);
        let whole = amount / factor;
        let frac = amount % factor;
        if frac == 0 {
            format!("{whole} {display_denom}")
        } else {
            let frac_str = format!("{:0>width$}", frac, width = decimals as usize)
                .trim_end_matches('0')
                .to_string();
            format!("{whole}.{frac_str} {display_denom}")
        }
    }

    fn sync_logs(&mut self) {
        // Pull any new log messages from the shared buffer
        let messages: Vec<String> = if let Ok(mut buffer) = self.log_buffer.lock() {
            buffer.drain(..).collect()
        } else {
            Vec::new()
        };

        for msg in messages {
            self.add_log(msg);
        }
    }

    fn add_log(&mut self, message: String) {
        self.logs.push(message);
        // Keep only last 20 log entries
        if self.logs.len() > 20 {
            self.logs.remove(0);
        }
    }

    fn player_hit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (spot, hand_in_spot, player_value, num_hands) = if let Some(ref mut game) = self.game_state {
            let spot = game.active_spot;
            let hand_in_spot = game.active_hand_in_spot;
            let num_hands = game.player_hands[spot].len();
            game.draw_card(false, Some(spot))?;
            let value = GameState::calculate_hand_value(&game.player_hands[spot][hand_in_spot]);
            (spot + 1, hand_in_spot + 1, value, num_hands)
        } else {
            return Ok(());
        };

        let hand_label = if num_hands > 1 {
            format!("Spot {spot}.{hand_in_spot}")
        } else {
            format!("Spot {spot}")
        };

        self.add_log(format!("{hand_label} hits"));

        if player_value > 21 {
            self.add_log(format!("{hand_label} busts with {player_value}!"));
            self.move_to_next_spot_or_dealer()?;
        } else if player_value == 21 {
            self.add_log(format!("{hand_label} has 21!"));
            self.move_to_next_spot_or_dealer()?;
        } else {
            self.add_log(format!("{hand_label} has {player_value}"));
        }

        Ok(())
    }

    fn player_stand(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref mut game) = self.game_state {
            let spot = game.active_spot;
            let hand_in_spot = game.active_hand_in_spot;
            game.hands_stood[spot][hand_in_spot] = true;

            let num_hands = game.player_hands[spot].len();
            let hand_label = if num_hands > 1 {
                format!("Spot {}.{}", spot + 1, hand_in_spot + 1)
            } else {
                format!("Spot {}", spot + 1)
            };
            self.add_log(format!("{hand_label} stands"));
        }

        self.move_to_next_spot_or_dealer()?;
        Ok(())
    }

    fn move_to_next_spot_or_dealer(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (has_next, active_spot, active_hand, num_spots) = if let Some(ref mut game) = self.game_state {
            let has_next = game.move_to_next_hand_or_spot();
            (has_next, game.active_spot, game.active_hand_in_spot, game.num_spots)
        } else {
            return Ok(());
        };

        if has_next {
            let num_hands_in_spot = if let Some(ref game) = self.game_state {
                game.player_hands[active_spot].len()
            } else {
                return Ok(());
            };
            // More hands/spots to play - check if new hand has 21
            let current_value = {
                let game = self.game_state.as_ref().unwrap();
                GameState::calculate_hand_value(&game.player_hands[active_spot][active_hand])
            };

            let hand_label = if num_hands_in_spot > 1 {
                format!("Spot {}.{}/{}", active_spot + 1, active_hand + 1, num_hands_in_spot)
            } else {
                format!("Spot {}/{}", active_spot + 1, num_spots)
            };

            if current_value == 21 {
                self.add_log(format!("{hand_label} has 21!"));
                // Mark as stood so it can't be played
                if let Some(ref mut game) = self.game_state {
                    game.hands_stood[active_spot][active_hand] = true;
                }
                // Recursively move to next hand/spot or dealer
                self.move_to_next_spot_or_dealer()?;
            } else {
                // Normal play - wait for user input
                let game = self.game_state.as_ref().unwrap();
                let can_double = game.can_double();
                let can_split = game.can_split();
                let can_surrender = game.can_surrender();

                let mut options = vec!["[H]it", "[S]tand"];
                if can_double { options.push("[D]ouble"); }
                if can_split { options.push("S[p]lit"); }
                if can_surrender { options.push("Su[r]render"); }

                self.status = format!("{}: {}", hand_label, options.join(" or "));
                self.add_log(format!("Playing {hand_label}"));
            }
        } else {
            // All spots/hands done, move to dealer
            self.phase = GamePhase::DealerTurn;
            self.dealer_play()?;
        }
        Ok(())
    }

    fn dealer_play(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.add_log("Dealer's turn...".to_string());

        loop {
            let should_hit = self.game_state.as_ref()
                .map(|g| g.dealer_should_hit())
                .unwrap_or(false);

            if !should_hit {
                break;
            }

            let dealer_value = if let Some(ref mut game) = self.game_state {
                game.draw_card(true, None)?;
                GameState::calculate_hand_value(&game.dealer_hand)
            } else {
                break;
            };

            self.add_log(format!("Dealer hits, has {dealer_value}"));

            if dealer_value > 21 {
                self.add_log(format!("Dealer busts with {dealer_value}!"));

                // All non-busted, non-surrendered hands win
                let hands_values: Vec<(usize, usize, u8, bool)> = if let Some(ref game) = self.game_state {
                    let mut values = Vec::new();
                    for (spot_idx, spot) in game.player_hands.iter().enumerate() {
                        for (hand_idx, hand) in spot.iter().enumerate() {
                            let surrendered = game.hands_surrendered[spot_idx][hand_idx];
                            values.push((spot_idx, hand_idx, GameState::calculate_hand_value(hand), surrendered));
                        }
                    }
                    values
                } else {
                    return Ok(());
                };

                self.spot_outcomes.clear();
                let mut wins = 0;
                let mut losses = 0;
                let mut surrenders = 0;

                // Resize spot_outcomes to match spots structure
                if let Some(ref game) = self.game_state {
                    self.spot_outcomes = game.player_hands.iter()
                        .map(|spot| vec![SpotOutcome::Push; spot.len()])
                        .collect();
                }

                for (spot_idx, hand_idx, player_value, surrendered) in hands_values {
                    let hand_label = if self.spot_outcomes[spot_idx].len() > 1 {
                        format!("Spot {}.{}", spot_idx + 1, hand_idx + 1)
                    } else {
                        format!("Spot {}", spot_idx + 1)
                    };

                    let outcome = if surrendered {
                        self.add_log(format!("{hand_label}: Surrendered (half loss)"));
                        surrenders += 1;
                        SpotOutcome::Surrender
                    } else if player_value > 21 {
                        self.add_log(format!("{hand_label}: Bust (loss)"));
                        losses += 1;
                        SpotOutcome::Loss
                    } else {
                        self.add_log(format!("{hand_label}: {player_value} - WIN"));
                        wins += 1;
                        SpotOutcome::Win
                    };
                    self.spot_outcomes[spot_idx][hand_idx] = outcome;
                }

                let status_msg = if surrenders > 0 {
                    format!("Dealer busts! {wins} wins, {losses} losses, {surrenders} surrenders. Press [N] for next game")
                } else {
                    format!("Dealer busts! {wins} wins, {losses} losses. Press [N] for next game")
                };
                self.status = status_msg;
                self.phase = GamePhase::GameOver;
                return Ok(());
            }
        }

        // Compare dealer hand against all player hands
        let (dealer_value, hands_values) = if let Some(ref game) = self.game_state {
            let dealer_value = GameState::calculate_hand_value(&game.dealer_hand);
            let mut values = Vec::new();
            for (spot_idx, spot) in game.player_hands.iter().enumerate() {
                for (hand_idx, hand) in spot.iter().enumerate() {
                    let surrendered = game.hands_surrendered[spot_idx][hand_idx];
                    values.push((spot_idx, hand_idx, GameState::calculate_hand_value(hand), surrendered));
                }
            }
            (dealer_value, values)
        } else {
            return Ok(());
        };

        self.add_log(format!("Dealer stands with {dealer_value}"));

        let mut wins = 0;
        let mut losses = 0;
        let mut pushes = 0;
        let mut surrenders = 0;

        // Clear previous outcomes and calculate new ones
        self.spot_outcomes.clear();

        // Resize spot_outcomes to match spots structure
        if let Some(ref game) = self.game_state {
            self.spot_outcomes = game.player_hands.iter()
                .map(|spot| vec![SpotOutcome::Push; spot.len()])
                .collect();
        }

        for (spot_idx, hand_idx, player_value, surrendered) in hands_values {
            let hand_label = if self.spot_outcomes[spot_idx].len() > 1 {
                format!("Spot {}.{}", spot_idx + 1, hand_idx + 1)
            } else {
                format!("Spot {}", spot_idx + 1)
            };

            let outcome = if surrendered {
                self.add_log(format!("{hand_label}: Surrendered (half loss)"));
                surrenders += 1;
                SpotOutcome::Surrender
            } else if player_value > 21 {
                self.add_log(format!("{hand_label}: Bust (loss)"));
                losses += 1;
                SpotOutcome::Loss
            } else if player_value > dealer_value {
                self.add_log(format!("{hand_label}: {player_value} vs {dealer_value} - WIN"));
                wins += 1;
                SpotOutcome::Win
            } else if dealer_value > player_value {
                self.add_log(format!("{hand_label}: {player_value} vs {dealer_value} - Loss"));
                losses += 1;
                SpotOutcome::Loss
            } else {
                self.add_log(format!("{hand_label}: {player_value} vs {dealer_value} - Push"));
                pushes += 1;
                SpotOutcome::Push
            };
            self.spot_outcomes[spot_idx][hand_idx] = outcome;
        }

        let status_msg = if surrenders > 0 {
            format!("Results: {wins} wins, {losses} losses, {pushes} pushes, {surrenders} surrenders. Press [N] for next game")
        } else {
            format!("Results: {wins} wins, {losses} losses, {pushes} pushes. Press [N] for next game")
        };
        self.status = status_msg;

        self.phase = GamePhase::GameOver;
        Ok(())
    }

    /// Display contract game payouts (sync — just formats strings).
    #[cfg(feature = "wallet")]
    fn display_contract_payouts(&mut self, game: &contract_msg::GameResponse) {
        self.add_log("=== Game Results ===".to_string());

        let dealer_value = calculate_hand_value_from_indices(&game.dealer_hand);
        self.add_log(format!("Dealer: {dealer_value}"));

        let mut total_payout_u128 = 0u128;

        for (idx, hand) in game.hands.iter().enumerate() {
            let player_value = calculate_hand_value_from_indices(&hand.cards);
            let hand_label = format!("Hand {}", idx + 1);

            let result = match hand.status.as_str() {
                "Won" => {
                    total_payout_u128 += hand.bet.u128();
                    format!("{hand_label}: {player_value} - WIN (+{})", hand.bet)
                }
                "Lost" => format!("{hand_label}: {player_value} - LOSS"),
                "Push" => format!("{hand_label}: {player_value} - PUSH"),
                "Surrendered" => {
                    let half_bet = hand.bet.u128() / 2;
                    format!("{hand_label}: Surrendered (-{half_bet})")
                }
                "Blackjack" => {
                    let blackjack_payout = hand.bet.u128() + (hand.bet.u128() * 3 / 2);
                    total_payout_u128 += blackjack_payout;
                    format!("{hand_label}: BLACKJACK! (+{blackjack_payout})")
                }
                _ => format!("{hand_label}: {}", hand.status),
            };

            self.add_log(result);
        }

        if total_payout_u128 > 0 {
            self.add_log(format!("Total payout: {total_payout_u128}"));
        }
    }

    // ── Spawn methods: launch background work, send Action when done ──

    #[cfg(feature = "wallet")]
    fn spawn_wallet_connect(&mut self) {
        if self.pending_op.is_some() { return; }
        let wallet = match self.wallet.as_mut() {
            Some(w) => w,
            None => return,
        };
        if wallet.client().is_some() { return; } // already connected

        let signer = wallet.signer();
        let chain_id = self.chain_id.clone();
        let rpc_url = self.rpc_url.clone();
        let tx = self.action_tx.clone();
        self.pending_op = Some("Connecting wallet".to_string());
        self.pending_op_start = Some(std::time::Instant::now());

        std::thread::spawn(move || {
            let config = mob::ChainConfig::new(chain_id, rpc_url, "xion".to_string());
            match mob::Client::new_with_signer(config, signer) {
                Ok(client) => { let _ = tx.send(Action::WalletConnected(client)); }
                Err(e) => { let _ = tx.send(Action::OpFailed { op_name: "Wallet connect".into(), error: e.to_string() }); }
            }
        });
    }

    #[cfg(feature = "wallet")]
    fn spawn_query_balance(&mut self) {
        if self.balance_poll_inflight { return; }
        let wallet = match self.wallet.as_ref() {
            Some(w) => w,
            None => return,
        };
        if wallet.client().is_none() { return; }
        let address = wallet.address().to_string();
        let rpc_url = wallet.client().unwrap().config().rpc_endpoint.clone();
        let tx = self.action_tx.clone();
        self.balance_poll_inflight = true;

        tokio::spawn(async move {
            match query_balance_standalone(&rpc_url, &address).await {
                Ok(balance_str) => { let _ = tx.send(Action::BalanceUpdated(balance_str)); }
                Err(e) => { log::debug!("Balance poll failed: {e}"); let _ = tx.send(Action::BalanceUpdated(String::new())); }
            }
        });
    }

    #[cfg(feature = "wallet")]
    fn spawn_list_games(&mut self) {
        if self.pending_op.is_some() { return; }
        let rpc_url = self.rpc_url.clone();
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => return,
        };
        let tx = self.action_tx.clone();
        self.pending_op = Some("Listing games".to_string());
        self.pending_op_start = Some(std::time::Instant::now());

        tokio::spawn(async move {
            match query_list_games_standalone(&rpc_url, &contract_addr, Some("WaitingForPlayerJoin".to_string())).await {
                Ok(games) => { let _ = tx.send(Action::GamesListed(games)); }
                Err(e) => { let _ = tx.send(Action::OpFailed { op_name: "List games".into(), error: e.to_string() }); }
            }
        });
    }

    #[cfg(feature = "wallet")]
    fn spawn_query_game_state(&mut self) {
        if self.game_poll_inflight { return; }
        let game_id = match self.game_id {
            Some(id) => id,
            None => return,
        };
        let rpc_url = self.rpc_url.clone();
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => return,
        };
        let tx = self.action_tx.clone();
        self.game_poll_inflight = true;

        tokio::spawn(async move {
            match query_game_by_id_standalone(&rpc_url, &contract_addr, game_id).await {
                Ok(game) => { let _ = tx.send(Action::GameStateUpdated(game)); }
                Err(e) => { log::debug!("Game poll failed: {e}"); let _ = tx.send(Action::OpFailed { op_name: "game_poll".into(), error: e.to_string() }); }
            }
        });
    }

    #[cfg(feature = "wallet")]
    fn spawn_join_game(&mut self) {
        if self.pending_op.is_some() { return; }
        if self.wallet.is_none() { return; }
        // Check prerequisites before taking client (avoids needing to return it on failure)
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => return,
        };
        let game_id = match self.game_id {
            Some(id) => id,
            None => { self.add_log("No game selected".into()); return; }
        };
        let wallet = self.wallet.as_mut().unwrap();
        let client = match wallet.take_client() {
            Some(c) => c,
            None => { self.add_log("Client not connected".into()); return; }
        };
        let rpc_url = self.rpc_url.clone();
        let log_buffer = Arc::clone(&self.log_buffer);
        let tx = self.action_tx.clone();

        self.pending_op = Some("Joining game".to_string());
        self.pending_op_start = Some(std::time::Instant::now());

        std::thread::spawn(move || {
            // Inner closure borrows client; thread keeps ownership so client survives errors.
            let result: Result<(zk_shuffle::babyjubjub::Fr, zk_shuffle::babyjubjub::Point), Box<dyn std::error::Error + Send + Sync>> = (|| {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let _rt_guard = rt.enter();

                push_log(&log_buffer, "Generating player keypair...");

                use zk_shuffle::elgamal::{KeyPair, Ciphertext};
                use zk_shuffle::shuffle::shuffle;
                use zk_shuffle::proof::{generate_shuffle_proof_rapidsnark, CanonicalSerialize, CanonicalDeserialize};
                use zk_shuffle::babyjubjub::Point;
                use ark_ec::{AffineRepr, CurveGroup};
                use rand_chacha::{ChaCha8Rng, rand_core::SeedableRng};

                let mut rng = ChaCha8Rng::from_entropy();
                let player_keys = KeyPair::generate(&mut rng);

                push_log(&log_buffer, &format!("Querying game {game_id}..."));
                let dealer_game: contract_msg::GameResponse = rt.block_on(
                    query_game_by_id_standalone(&rpc_url, &contract_addr, game_id)
                )?;

                let dealer_shuffled = dealer_game.player_shuffled_deck
                    .ok_or("Dealer hasn't shuffled deck yet")?;
                push_log(&log_buffer, &format!("Retrieved dealer's shuffled deck ({} cards)", dealer_shuffled.len()));

                let dealer_deck: Vec<Ciphertext> = dealer_shuffled.iter()
                    .map(|binary| {
                        let mut cursor = binary.as_slice();
                        let c0 = Point::deserialize_compressed(&mut cursor)
                            .map_err(|e| format!("Failed to deserialize c0: {e}"))?;
                        let c1 = Point::deserialize_compressed(&mut cursor)
                            .map_err(|e| format!("Failed to deserialize c1: {e}"))?;
                        Ok(Ciphertext { c0, c1 })
                    })
                    .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;

                let dealer_pk = Point::deserialize_compressed(&mut dealer_game.dealer_pubkey.as_slice())
                    .map_err(|e| format!("Failed to deserialize dealer pubkey: {e}"))?;

                let aggregated_pk = (player_keys.pk.into_group() + dealer_pk.into_group()).into_affine();

                push_log(&log_buffer, "Shuffling deck...");
                let player_shuffle = shuffle(&mut rng, &dealer_deck, &aggregated_pk);

                push_log(&log_buffer, "Generating ZK proof (this may take ~1 minute)...");
                let player_proof = generate_shuffle_proof_rapidsnark(
                    &player_shuffle.public_inputs,
                    player_shuffle.private_inputs,
                ).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;
                push_log(&log_buffer, "Proof generated!");

                let serialize_point = |p: &Point| -> Vec<u8> {
                    let mut buf = Vec::new();
                    p.serialize_compressed(&mut buf).unwrap();
                    buf
                };
                let serialize_ciphertext = |ct: &Ciphertext| -> Vec<u8> {
                    let mut buf = Vec::new();
                    ct.c0.serialize_compressed(&mut buf).unwrap();
                    ct.c1.serialize_compressed(&mut buf).unwrap();
                    buf
                };

                let proof_json = serde_json::to_string(&player_proof)?;
                let public_inputs_strs: Vec<String> = player_shuffle.public_inputs
                    .to_ark_public_inputs().iter()
                    .map(|f| {
                        let bigint = num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le());
                        bigint.to_string()
                    }).collect();

                let config: contract_msg::Config = rt.block_on(query_config_standalone(&rpc_url, &contract_addr))?;
                let bet_amount = config.min_bet.u128();
                let denom = config.denom;

                let msg_json = serde_json::json!({
                    "join_game": {
                        "bet": bet_amount.to_string(),
                        "public_key": general_purpose::STANDARD.encode(serialize_point(&player_keys.pk)),
                        "shuffled_deck": player_shuffle.deck.iter().map(|ct| general_purpose::STANDARD.encode(serialize_ciphertext(ct))).collect::<Vec<_>>(),
                        "proof": general_purpose::STANDARD.encode(&proof_json),
                        "public_inputs": public_inputs_strs,
                    }
                });
                let msg_bytes = serde_json::to_vec(&msg_json)?;

                push_log(&log_buffer, &format!("Submitting transaction with {} {denom}...", bet_amount));

                let funds = vec![mob::Coin { denom, amount: bet_amount.to_string() }];
                let tx_response = execute_and_confirm_standalone(&client, contract_addr, msg_bytes, funds, "Join blackjack game", None)?;

                if tx_response.code != 0 {
                    return Err(format!("Transaction failed: {}", tx_response.raw_log).into());
                }
                push_log(&log_buffer, &format!("Transaction confirmed! Hash: {}", tx_response.txhash));
                Ok((player_keys.sk, player_keys.pk))
            })();

            match result {
                Ok((sk, pk)) => { let _ = tx.send(Action::GameJoined { client, sk, pk }); }
                Err(e) => { let _ = tx.send(Action::TxFailed { action_name: "Join game".into(), client, error: e.to_string() }); }
            }
        });
    }

    /// Spawn a simple contract TX (hit, stand, surrender, claim_timeout).
    #[cfg(feature = "wallet")]
    fn spawn_simple_tx(&mut self, action_name: &str, msg_json: serde_json::Value) {
        if self.pending_op.is_some() { return; }
        let wallet = match self.wallet.as_mut() {
            Some(w) => w,
            None => return,
        };
        let client = match wallet.take_client() {
            Some(c) => c,
            None => { self.add_log("Client not connected".into()); return; }
        };
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => { wallet.set_client(client); return; }
        };
        let tx = self.action_tx.clone();
        let name = action_name.to_string();
        self.pending_op = Some(name.clone());
        self.pending_op_start = Some(std::time::Instant::now());
        self.add_log(format!("Submitting {name}..."));

        std::thread::spawn(move || {
            let msg_bytes = serde_json::to_vec(&msg_json).unwrap();
            match execute_and_confirm_standalone(&client, contract_addr, msg_bytes, vec![], &name, None) {
                Ok(resp) if resp.code == 0 => {
                    let _ = tx.send(Action::TxCompleted { action_name: name, client, txhash: resp.txhash });
                }
                Ok(resp) => {
                    let _ = tx.send(Action::TxFailed { action_name: name, client, error: resp.raw_log });
                }
                Err(e) => {
                    let _ = tx.send(Action::TxFailed { action_name: name, client, error: e.to_string() });
                }
            }
        });
    }

    /// Spawn double_down or split (needs funds query).
    #[cfg(feature = "wallet")]
    fn spawn_funded_tx(&mut self, action_name: &str, msg_json: serde_json::Value) {
        if self.pending_op.is_some() { return; }
        let wallet = match self.wallet.as_mut() {
            Some(w) => w,
            None => return,
        };
        let client = match wallet.take_client() {
            Some(c) => c,
            None => { self.add_log("Client not connected".into()); return; }
        };
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => { wallet.set_client(client); return; }
        };
        let game_id = match self.game_id {
            Some(id) => id,
            None => { wallet.set_client(client); return; }
        };
        let rpc_url = self.rpc_url.clone();
        let tx = self.action_tx.clone();
        let name = action_name.to_string();
        self.pending_op = Some(name.clone());
        self.pending_op_start = Some(std::time::Instant::now());
        self.add_log(format!("Submitting {name}..."));

        std::thread::spawn(move || {
            let result = (|| -> Result<mob::TxResponse, Box<dyn std::error::Error + Send + Sync>> {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let _rt_guard = rt.enter();
                let config: contract_msg::Config = rt.block_on(query_config_standalone(&rpc_url, &contract_addr))?;
                let game: contract_msg::GameResponse = rt.block_on(query_game_by_id_standalone(&rpc_url, &contract_addr, game_id))?;
                let funds = vec![mob::Coin { denom: config.denom, amount: game.bet.to_string() }];
                let msg_bytes = serde_json::to_vec(&msg_json)?;
                execute_and_confirm_standalone(&client, contract_addr, msg_bytes, funds, &name, None)
            })();

            match result {
                Ok(resp) if resp.code == 0 => {
                    let _ = tx.send(Action::TxCompleted { action_name: name, client, txhash: resp.txhash });
                }
                Ok(resp) => {
                    let _ = tx.send(Action::TxFailed { action_name: name, client, error: resp.raw_log });
                }
                Err(e) => {
                    let _ = tx.send(Action::TxFailed { action_name: name, client, error: e.to_string() });
                }
            }
        });
    }

    #[cfg(feature = "wallet")]
    fn spawn_hit(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_simple_tx("Hit", serde_json::json!({ "hit": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_stand(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_simple_tx("Stand", serde_json::json!({ "stand": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_surrender(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_simple_tx("Surrender", serde_json::json!({ "surrender": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_claim_timeout(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_simple_tx("Claim Timeout", serde_json::json!({ "claim_timeout": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_double_down(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_funded_tx("Double Down", serde_json::json!({ "double_down": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_split(&mut self) {
        let game_id = match self.game_id { Some(id) => id, None => return };
        self.spawn_funded_tx("Split", serde_json::json!({ "split": { "game_id": game_id } }));
    }

    #[cfg(feature = "wallet")]
    fn spawn_submit_reveal(&mut self, card_index: u32) {
        if self.pending_op.is_some() { return; }
        if self.wallet.is_none() { return; }
        // Check all prerequisites before taking client
        let contract_addr = match self.contract_address.clone() {
            Some(a) => a,
            None => return,
        };
        let game_id = match self.game_id {
            Some(id) => id,
            None => return,
        };
        let (sk, pk) = match self.zk_keys {
            Some(keys) => keys,
            None => { self.add_log("ZK keys not initialized".into()); return; }
        };
        let wallet = self.wallet.as_mut().unwrap();
        let client = match wallet.take_client() {
            Some(c) => c,
            None => { self.add_log("Client not connected".into()); return; }
        };

        let rpc_url = self.rpc_url.clone();
        let log_buffer = Arc::clone(&self.log_buffer);
        let tx = self.action_tx.clone();

        self.pending_op = Some(format!("Revealing card {card_index}"));
        self.pending_op_start = Some(std::time::Instant::now());
        self.add_log(format!("Submitting reveal for card {card_index}..."));

        std::thread::spawn(move || {
            // Inner closure borrows client; thread keeps ownership so client survives errors.
            let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = (|| {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let _rt_guard = rt.enter();

                let game: contract_msg::GameResponse = rt.block_on(
                    query_game_by_id_standalone(&rpc_url, &contract_addr, game_id)
                )?;

                if card_index as usize >= game.deck.len() {
                    return Err(format!("Invalid card_index: {card_index}").into());
                }

                use zk_shuffle::decrypt::reveal_card;
                use zk_shuffle::proof::{generate_reveal_proof_rapidsnark, CanonicalSerialize, CanonicalDeserialize};
                use zk_shuffle::elgamal::Ciphertext;
                use zk_shuffle::babyjubjub::Point;

                let card_binary = &game.deck[card_index as usize];
                let mut cursor = card_binary.as_slice();
                let c0 = Point::deserialize_compressed(&mut cursor)
                    .map_err(|e| format!("Failed to deserialize card c0: {e}"))?;
                let c1 = Point::deserialize_compressed(&mut cursor)
                    .map_err(|e| format!("Failed to deserialize card c1: {e}"))?;
                let encrypted_card = Ciphertext { c0, c1 };

                push_log(&log_buffer, &format!("Revealing card {card_index}..."));
                let reveal = reveal_card(&sk, &encrypted_card, &pk);

                push_log(&log_buffer, "Generating reveal proof...");
                let reveal_proof = generate_reveal_proof_rapidsnark(&reveal.public_inputs, reveal.sk_p)
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;

                let mut partial_buf = Vec::new();
                reveal.partial_decryption.serialize_compressed(&mut partial_buf)
                    .map_err(|e| format!("Failed to serialize partial decryption: {e}"))?;
                let proof_json = serde_json::to_string(&reveal_proof)?;
                let public_inputs_strs: Vec<String> = reveal.public_inputs
                    .to_ark_public_inputs().iter()
                    .map(|f| {
                        let bigint = num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le());
                        bigint.to_string()
                    }).collect();

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

                push_log(&log_buffer, "Submitting reveal to contract...");
                let tx_response = execute_and_confirm_standalone(&client, contract_addr, msg_bytes, vec![], "Submit Reveal", None)?;

                if tx_response.code != 0 {
                    return Err(format!("Transaction failed: {}", tx_response.raw_log).into());
                }
                push_log(&log_buffer, &format!("Reveal confirmed! Hash: {}", tx_response.txhash));
                Ok(())
            })();

            match result {
                Ok(()) => { let _ = tx.send(Action::RevealSubmitted { client, card_index }); }
                Err(e) => { let _ = tx.send(Action::TxFailed { action_name: "Submit Reveal".into(), client, error: e.to_string() }); }
            }
        });
    }

    // ── Action handling ──

    #[cfg(feature = "wallet")]
    fn handle_actions(&mut self) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                Action::WalletConnected(client) => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if let Some(ref mut wallet) = self.wallet {
                        wallet.set_client(client);
                    }
                    self.add_log("Wallet connected to RPC".into());
                    // Trigger first balance poll
                    self.spawn_query_balance();
                }
                Action::BalanceUpdated(balance_str) => {
                    self.balance_poll_inflight = false;
                    if !balance_str.is_empty() {
                        self.wallet_balance = Some(balance_str);
                    }
                    self.last_balance_poll = Some(std::time::Instant::now());
                }
                Action::GamesListed(games) => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if games.is_empty() {
                        self.add_log("No games available to join".into());
                        self.status = "No games available. Press [L] to refresh".into();
                    } else {
                        let game_count = games.len();
                        for (idx, game) in games.iter().enumerate() {
                            self.add_log(format!("  [{}] Game ID: {} - Dealer: {}", idx, game.game_id, game.dealer));
                        }
                        self.available_games = games;
                        self.add_log(format!("Found {game_count} available games"));
                        self.add_log("Press number key to select game, then [J] to join".into());
                        self.status = "Select game by number".into();
                    }
                }
                Action::GameStateUpdated(game) => {
                    self.game_poll_inflight = false;
                    self.process_game_state_update(game);
                }
                Action::GameJoined { client, sk, pk } => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if let Some(ref mut wallet) = self.wallet {
                        wallet.set_client(client);
                    }
                    self.zk_keys = Some((sk, pk));
                    self.phase = GamePhase::WaitingForReveal;
                    self.status = "Game started! Waiting for reveals...".into();
                }
                Action::TxCompleted { action_name, client, txhash } => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if let Some(ref mut wallet) = self.wallet {
                        wallet.set_client(client);
                    }
                    self.add_log(format!("{action_name} confirmed! Hash: {txhash}"));
                    // Transition based on action
                    match action_name.as_str() {
                        "Hit" | "Double Down" | "Split" => {
                            self.phase = GamePhase::WaitingForReveal;
                            self.status = "Waiting for card reveal...".into();
                        }
                        "Stand" | "Surrender" => {
                            // Game may transition — poll will pick it up
                            self.phase = GamePhase::WaitingForReveal;
                            self.status = "Waiting for game update...".into();
                        }
                        "Claim Timeout" => {
                            self.phase = GamePhase::GameOver;
                            self.status = "Game ended due to timeout. Press [N] for next game".into();
                        }
                        _ => {}
                    }
                }
                Action::RevealSubmitted { client, card_index } => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if let Some(ref mut wallet) = self.wallet {
                        wallet.set_client(client);
                    }
                    self.add_log(format!("Reveal for card {card_index} submitted"));
                }
                Action::TxFailed { action_name, client, error } => {
                    self.pending_op = None;
                    self.pending_op_start = None;
                    if let Some(ref mut wallet) = self.wallet {
                        wallet.set_client(client);
                    }
                    self.add_log(format!("{action_name} failed: {error}"));
                }
                Action::OpFailed { op_name, error } => {
                    if op_name == "game_poll" {
                        self.game_poll_inflight = false;
                    } else {
                        self.pending_op = None;
                        self.pending_op_start = None;
                    }
                    self.add_log(format!("{op_name} failed: {error}"));
                }
            }
        }

        // Check for stuck operations (60s timeout)
        #[cfg(feature = "wallet")]
        if let Some(start) = self.pending_op_start {
            if start.elapsed() > std::time::Duration::from_secs(60) {
                if let Some(op) = self.pending_op.take() {
                    self.add_log(format!("WARNING: {op} timed out after 60s"));
                }
                self.pending_op_start = None;
            }
        }
    }

    #[cfg(feature = "wallet")]
    fn process_game_state_update(&mut self, game: contract_msg::GameResponse) {
        self.contract_game_state = Some(game.clone());

        // WaitingForReveal — submit reveals and check completion
        if self.phase == GamePhase::WaitingForReveal {
            // Parse reveal_requests from status string (e.g. "WaitingForReveal { reveal_requests: [0, 1, 2], ... }")
            let reveal_requests: Vec<u32> = if game.status.contains("reveal_requests:") {
                game.status
                    .split("reveal_requests:")
                    .nth(1)
                    .and_then(|s| s.split(']').next())
                    .map(|s| s.trim_start_matches(|c: char| c == ' ' || c == '['))
                    .map(|s| {
                        s.split(',')
                            .filter_map(|n| n.trim().parse::<u32>().ok())
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            };

            // Submit one missing reveal at a time (only if no pending op)
            if self.pending_op.is_none() {
                // First: check reveal_requests for cards we haven't submitted to yet
                // (pending_reveals only has entries after the first party submits)
                let already_submitted: Vec<u32> = game.pending_reveals.iter()
                    .filter(|pr| pr.player_partial.is_some())
                    .map(|pr| pr.card_index)
                    .collect();

                let mut submitted = false;
                for &card_idx in &reveal_requests {
                    if !already_submitted.contains(&card_idx) {
                        self.spawn_submit_reveal(card_idx);
                        submitted = true;
                        break;
                    }
                }

                // Fallback: check pending_reveals for cards still missing our partial
                if !submitted {
                    for pending in &game.pending_reveals {
                        if pending.player_partial.is_none() {
                            self.spawn_submit_reveal(pending.card_index);
                            break;
                        }
                    }
                }
            }

            // Check if game has transitioned past WaitingForReveal
            if !game.status.contains("WaitingForReveal") {
                if game.status.contains("PlayerTurn") {
                    self.phase = GamePhase::PlayerTurn;
                    self.status = "Your turn! [H]it, [S]tand, etc.".into();
                } else if game.status.contains("DealerTurn") {
                    self.phase = GamePhase::DealerTurn;
                    self.status = "Dealer's turn...".into();
                } else if game.status.contains("Settled") {
                    self.phase = GamePhase::GameOver;
                    self.display_contract_payouts(&game);
                }
            } else {
                let total = reveal_requests.len();
                let done = game.pending_reveals.iter()
                    .filter(|pr| pr.player_partial.is_some() && pr.dealer_partial.is_some())
                    .count();
                self.status = format!("Waiting for reveals... ({}/{})", done, total);
            }
        }

        // DealerTurn in contract mode — just poll
        if self.phase == GamePhase::DealerTurn && self.selected_mode == Some(GameMode::Contract) {
            self.status = format!("Dealer's turn... (status: {})", game.status);
            if game.status.contains("Settled") {
                self.phase = GamePhase::GameOver;
                self.display_contract_payouts(&game);
            }
        }
    }
}

// ── Standalone free functions (no dependency on App or mob Client for queries) ──

#[cfg(feature = "wallet")]
fn push_log(log_buffer: &Arc<Mutex<Vec<String>>>, msg: &str) {
    if let Ok(mut buf) = log_buffer.lock() {
        buf.push(msg.to_string());
    }
}

#[cfg(feature = "wallet")]
async fn query_contract_raw_standalone(
    rpc_url: &str,
    contract_addr: &str,
    query_msg: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    use tendermint_rpc::{Client as TmClient, HttpClient};
    use prost::Message;

    let path = "/cosmwasm.wasm.v1.Query/SmartContractState";
    let data = {
        let req = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateRequest {
            address: contract_addr.to_string(),
            query_data: query_msg.to_vec(),
        };
        req.encode_to_vec()
    };

    let tm_client = HttpClient::new(rpc_url)?;
    let response = tm_client.abci_query(Some(path.to_string()), data, None, false).await?;

    if response.code.is_err() {
        return Err(format!("ABCI query failed: {}", response.log).into());
    }

    let res_wrapper = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateResponse::decode(response.value.as_slice())?;
    Ok(res_wrapper.data)
}

#[cfg(feature = "wallet")]
async fn query_balance_standalone(
    rpc_url: &str,
    address: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use tendermint_rpc::{Client as TmClient, HttpClient};
    use prost::Message;

    let path = "/cosmos.bank.v1beta1.Query/AllBalances";
    let data = {
        let req = xion_types::cosmos::bank::v1beta1::QueryAllBalancesRequest {
            address: address.to_string(),
            pagination: None,
            resolve_denom: false,
        };
        req.encode_to_vec()
    };

    let tm_client = HttpClient::new(rpc_url)?;
    let response = tm_client.abci_query(Some(path.to_string()), data, None, false).await?;

    if response.code.is_err() {
        return Err(format!("Balance query failed: {}", response.log).into());
    }

    let balance_response = xion_types::cosmos::bank::v1beta1::QueryAllBalancesResponse::decode(response.value.as_slice())?;

    if balance_response.balances.is_empty() {
        Ok("0 xion".to_string())
    } else {
        let balances: Vec<String> = balance_response.balances.iter()
            .map(|coin| {
                let amount: u128 = coin.amount.parse().unwrap_or(0);
                App::format_denom(amount, &coin.denom)
            })
            .collect();
        Ok(balances.join(", "))
    }
}

#[cfg(feature = "wallet")]
async fn query_config_standalone(
    rpc_url: &str,
    contract_addr: &str,
) -> Result<contract_msg::Config, Box<dyn std::error::Error + Send + Sync>> {
    let query_bytes = serde_json::to_vec(&serde_json::json!({ "get_config": {} }))?;
    let response_bytes = query_contract_raw_standalone(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

#[cfg(feature = "wallet")]
async fn query_game_by_id_standalone(
    rpc_url: &str,
    contract_addr: &str,
    game_id: u64,
) -> Result<contract_msg::GameResponse, Box<dyn std::error::Error + Send + Sync>> {
    let query_bytes = serde_json::to_vec(&serde_json::json!({ "get_game": { "game_id": game_id } }))?;
    let response_bytes = query_contract_raw_standalone(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

#[cfg(feature = "wallet")]
async fn query_list_games_standalone(
    rpc_url: &str,
    contract_addr: &str,
    status_filter: Option<String>,
) -> Result<Vec<contract_msg::GameListItem>, Box<dyn std::error::Error + Send + Sync>> {
    let query_bytes = serde_json::to_vec(&serde_json::json!({ "list_games": { "status_filter": status_filter } }))?;
    let response_bytes = query_contract_raw_standalone(rpc_url, contract_addr, &query_bytes).await?;
    Ok(serde_json::from_slice(&response_bytes)?)
}

/// Convert 0-51 card indices to blackjack hand value (with ace soft/hard logic).
fn calculate_hand_value_from_indices(indices: &[u8]) -> u8 {
    let mut total: u8 = 0;
    let mut aces: u8 = 0;
    for &idx in indices {
        let card = blackjack::Card::from_index(idx as usize);
        let v = card.value();
        total = total.saturating_add(v);
        if v == 11 {
            aces += 1;
        }
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    total
}

/// Execute a contract message via mob Client + poll for confirmation.
/// Must be called from a non-tokio thread (mob creates its own runtime).
#[cfg(feature = "wallet")]
fn execute_and_confirm_standalone(
    client: &mob::Client,
    contract_addr: String,
    msg_bytes: Vec<u8>,
    funds: Vec<mob::Coin>,
    memo: &str,
    gas_limit: Option<u64>,
) -> Result<mob::TxResponse, Box<dyn std::error::Error + Send + Sync>> {
    let broadcast = client.execute_contract(
        contract_addr,
        msg_bytes,
        funds,
        Some(memo.to_string()),
        gas_limit,
    )?;

    if broadcast.code != 0 {
        return Err(format!("Broadcast rejected: {}", broadcast.raw_log).into());
    }

    // Poll for confirmation
    for attempt in 0..15 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        match client.get_tx(broadcast.txhash.clone()) {
            Ok(tx) => return Ok(tx),
            Err(_) if attempt < 14 => continue,
            Err(e) => return Err(format!("Tx not found after polling: {e}").into()),
        }
    }
    Err("Tx not confirmed after 30s".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    // Initialize custom logger
    let (logger, log_buffer) = TuiLogger::new();
    log::set_boxed_logger(Box::new(logger))
        .map(|()| log::set_max_level(log::LevelFilter::Info))
        .expect("Failed to initialize logger");

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new(log_buffer);
    let res = run_app(&mut terminal, app).await;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}")
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<(), Box<dyn Error>>
where
    B::Error: 'static,
{
    loop {
        // Sync any new log messages from the logger
        app.sync_logs();

        // Drain action channel
        #[cfg(feature = "wallet")]
        app.handle_actions();

        // Render UI
        terminal.draw(|f| ui(f, &app))?;

        // Non-blocking balance polling every 10s
        #[cfg(feature = "wallet")]
        if app.wallet.as_ref().map_or(false, |w| w.client().is_some()) {
            let should_poll = app.last_balance_poll
                .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(10));
            if should_poll {
                app.spawn_query_balance();
            }
        }

        // Non-blocking game state polling every 2s
        #[cfg(feature = "wallet")]
        if matches!(app.phase, GamePhase::WaitingForReveal | GamePhase::DealerTurn) {
            if app.game_id.is_some() {
                let should_poll = app.last_game_poll
                    .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(2));
                if should_poll {
                    app.last_game_poll = Some(std::time::Instant::now());
                    app.spawn_query_game_state();
                }
            }
        }

        // Update loading animation
        #[cfg(feature = "wallet")]
        if app.pending_op.is_some() {
            app.loading_dots = (app.loading_dots + 1) % 4;
            if let Some(ref op) = app.pending_op {
                let dots = ".".repeat(app.loading_dots);
                if let Some(start) = app.pending_op_start {
                    let elapsed = start.elapsed().as_secs();
                    app.status = format!("{op} {elapsed}s{dots:<3}");
                }
            }
        }

        if app.phase == GamePhase::Initializing {
            app.loading_dots = (app.loading_dots + 1) % 4;
            let dots = ".".repeat(app.loading_dots);

            // Update stage based on recent logs
            if !app.logs.is_empty() {
                let last_log = &app.logs[app.logs.len() - 1];
                let new_stage = if last_log.contains("Loading proving/verifying keys") {
                    Some("Loading keys")
                } else if last_log.contains("Using cached proving keys") {
                    Some("Using cached keys")
                } else if last_log.contains("Proving key loaded")
                    || last_log.contains("Initializing deck")
                    || last_log.contains("Shuffling deck")
                {
                    Some("Shuffling")
                } else if last_log.contains("Generating player shuffle proof") {
                    Some("Player shuffle proof")
                } else if last_log.contains("Generating dealer shuffle proof") {
                    Some("Dealer shuffle proof")
                } else {
                    None
                };

                if let Some(stage) = new_stage {
                    if app.current_init_stage != stage {
                        if !app.current_init_stage.is_empty() {
                            if let Some(start_time) = app.init_start_time {
                                let elapsed = start_time.elapsed().as_secs();
                                app.add_log(format!(">>> {} completed in {}s <<<", app.current_init_stage, elapsed));
                            }
                        }

                        app.current_init_stage = stage.to_string();
                        app.init_start_time = Some(std::time::Instant::now());
                        app.add_log(format!(">>> {stage} started <<<"));
                    }
                }
            }

            if let Some(start_time) = app.init_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if !app.current_init_stage.is_empty() {
                    app.status = format!("{} {}s{:<3}", app.current_init_stage, elapsed, dots);
                } else if app.selected_mode == Some(GameMode::Trustless) {
                    app.status = format!("Initializing{elapsed}s{dots:<3}");
                } else if app.selected_mode == Some(GameMode::Fast) {
                    app.status = format!("Creating fast game{dots:<3}");
                }
            }
        }

        // Start deck initialization and shuffle after mode selection (before spots chosen)
        if app.phase == GamePhase::SpotSelection && app.selected_mode.is_some() && app.init_task.is_none() {
            app.init_start_time = Some(std::time::Instant::now());
            app.current_init_stage = "Initializing".to_string();
            let mode = app.selected_mode.unwrap();

            let task = tokio::task::spawn(async move {
                let mut game_state = GameState::new_uninitialized(mode).await
                    .map_err(|e| e.to_string())?;

                game_state.initialize_deck()
                    .map_err(|e| e.to_string())?;
                game_state.shuffle_deck()
                    .map_err(|e| e.to_string())?;

                Ok(game_state)
            });
            app.init_task = Some(task);
        }

        // After spots selected, resize game state and deal cards
        if app.phase == GamePhase::Initializing && app.selected_spots.is_some() && app.game_state.is_none() {
            if let Some(task) = &app.init_task {
                if task.is_finished() {
                    let task = app.init_task.take().unwrap();
                    match task.await {
                        Ok(Ok(mut game_state)) => {
                            let num_spots = app.selected_spots.unwrap();

                            if let Err(e) = game_state.resize_for_spots(num_spots) {
                                app.add_log(format!("ERROR: {e}"));
                                app.status = "Error setting spots. Press [F] or [T] to try again".to_string();
                                app.phase = GamePhase::ModeSelection;
                                app.selected_mode = None;
                                app.selected_spots = None;
                                app.init_start_time = None;
                                app.current_init_stage.clear();
                            } else {
                                for spot in 0..num_spots {
                                    if let Err(e) = game_state.draw_card(false, Some(spot)) {
                                        app.add_log(format!("Error dealing: {e}"));
                                    }
                                }
                                if let Err(e) = game_state.draw_card(true, None) {
                                    app.add_log(format!("Error dealing: {e}"));
                                }
                                for spot in 0..num_spots {
                                    if let Err(e) = game_state.draw_card(false, Some(spot)) {
                                        app.add_log(format!("Error dealing: {e}"));
                                    }
                                }
                                if let Err(e) = game_state.draw_card(true, None) {
                                    app.add_log(format!("Error dealing: {e}"));
                                }

                                app.game_state = Some(game_state);
                                app.phase = GamePhase::PlayerTurn;
                                app.init_start_time = None;
                                app.current_init_stage.clear();

                                let should_peek = app.game_state.as_ref().map(|g| g.should_dealer_peek()).unwrap_or(false);
                                if should_peek {
                                    if let Some(ref mut game) = app.game_state {
                                        game.dealer_peeked = true;
                                    }
                                    let has_blackjack = app.game_state.as_ref().map(|g| g.dealer_has_blackjack()).unwrap_or(false);
                                    if has_blackjack {
                                        app.add_log("Dealer peeks and has Blackjack!".to_string());
                                        if let Err(e) = app.dealer_play() {
                                            app.add_log(format!("Error: {e}"));
                                        }
                                        return Ok(());
                                    } else {
                                        app.add_log("Dealer peeks - no Blackjack".to_string());
                                    }
                                }

                                let first_spot_value = {
                                    let game = app.game_state.as_ref().unwrap();
                                    GameState::calculate_hand_value(&game.player_hands[0][0])
                                };

                                if first_spot_value == 21 {
                                    app.add_log(format!("Game started! Spot 1/{num_spots} has Blackjack!"));
                                    if let Some(ref mut game) = app.game_state {
                                        game.hands_stood[0][0] = true;
                                    }
                                    if let Err(e) = app.move_to_next_spot_or_dealer() {
                                        app.add_log(format!("Error: {e}"));
                                    }
                                } else {
                                    let game = app.game_state.as_ref().unwrap();
                                    let can_double = game.can_double();
                                    let can_split = game.can_split();
                                    let can_surrender = game.can_surrender();
                                    let mut options = vec!["[H]it", "[S]tand"];
                                    if can_double { options.push("[D]ouble"); }
                                    if can_split { options.push("S[p]lit"); }
                                    if can_surrender { options.push("Su[r]render"); }
                                    app.status = format!("Spot {}/{}: {}", game.active_spot + 1, game.num_spots, options.join(" or "));
                                    app.add_log(format!("Game started! Playing spot 1/{}", game.num_spots));
                                }

                                let mode = app.selected_mode.unwrap();
                                let num_spots = app.selected_spots.unwrap();
                                app.add_log("Background: Pre-shuffling next game...".to_string());
                                let next_task = tokio::task::spawn(async move {
                                    let mut next_game = GameState::new(mode, num_spots).await
                                        .map_err(|e| e.to_string())?;
                                    next_game.initialize_deck()
                                        .map_err(|e| e.to_string())?;
                                    next_game.shuffle_deck()
                                        .map_err(|e| e.to_string())?;
                                    log::info!("Background: Next game shuffled and ready!");
                                    Ok(next_game)
                                });
                                app.next_game_task = Some(next_task);
                            }
                        }
                        Ok(Err(e)) => {
                            app.add_log(format!("ERROR: {e}"));
                            app.status = "Error starting game. Press [F] or [T] to try again".to_string();
                            app.phase = GamePhase::ModeSelection;
                            app.selected_mode = None;
                            app.selected_spots = None;
                            app.init_start_time = None;
                            app.current_init_stage.clear();
                        }
                        Err(e) => {
                            app.add_log(format!("Task error: {e}"));
                            app.status = "Error starting game. Press [F] or [T] to try again".to_string();
                            app.phase = GamePhase::ModeSelection;
                            app.selected_mode = None;
                            app.selected_spots = None;
                            app.init_start_time = None;
                            app.current_init_stage.clear();
                        }
                    }
                }
            }
        }

        // Use poll with timeout so UI can refresh even during long operations
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Handle Esc to quit globally
                if matches!(key.code, KeyCode::Esc) {
                    if let Some(task) = app.init_task.take() {
                        task.abort();
                    }
                    if let Some(task) = app.next_game_task.take() {
                        task.abort();
                    }
                    return Ok(());
                }

                // Allow ? and Q even during pending operations
                if matches!(key.code, KeyCode::Char('?')) {
                    app.help_visible = !app.help_visible;
                    continue;
                }
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) && app.input_mode == InputMode::Normal {
                    if let Some(task) = app.init_task.take() { task.abort(); }
                    if let Some(task) = app.next_game_task.take() { task.abort(); }
                    return Ok(());
                }

                // Block action keys while pending_op is active (except input modes)
                #[cfg(feature = "wallet")]
                if app.pending_op.is_some() && app.input_mode == InputMode::Normal {
                    continue;
                }

                match app.input_mode {
                    #[cfg(feature = "wallet")]
                    InputMode::Mnemonic => {
                        match key.code {
                            KeyCode::Char(c) => app.mnemonic_input.push(c),
                            KeyCode::Backspace => { app.mnemonic_input.pop(); }
                            KeyCode::Enter => {
                                let mnemonic = app.mnemonic_input.clone();
                                app.mnemonic_input.clear();
                                if app.load_wallet_from_mnemonic(&mnemonic) {
                                    app.spawn_wallet_connect();
                                }
                            }
                            _ => {}
                        }
                    }
                    #[cfg(feature = "wallet")]
                    InputMode::ContractAddress => {
                        match key.code {
                            KeyCode::Char(c) => app.contract_address_input.push(c),
                            KeyCode::Backspace => { app.contract_address_input.pop(); }
                            KeyCode::Enter => {
                                if app.contract_address_input.is_empty() {
                                    app.add_log("Please enter a contract address".to_string());
                                } else {
                                    let addr = app.contract_address_input.clone();
                                    app.contract_address = Some(addr.clone());
                                    app.add_log(format!("Contract address set: {addr}"));
                                    app.contract_address_input.clear();
                                    app.input_mode = InputMode::Normal;

                                    if app.wallet.as_ref().map_or(true, |w| w.client().is_none()) {
                                        app.spawn_wallet_connect();
                                    } else {
                                        app.spawn_list_games();
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    InputMode::Normal => match key.code {
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    if matches!(app.phase, GamePhase::ModeSelection | GamePhase::GameOver) {
                        app.selected_mode = Some(GameMode::Fast);
                        app.phase = GamePhase::SpotSelection;
                        app.add_log("FAST mode selected".to_string());
                        app.add_log("Initializing deck in background...".to_string());
                        app.status = "Select number of spots (1-8):".to_string();
                    }
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    if matches!(app.phase, GamePhase::ModeSelection | GamePhase::GameOver) {
                        app.selected_mode = Some(GameMode::Trustless);
                        app.phase = GamePhase::SpotSelection;
                        app.add_log("TRUSTLESS mode selected".to_string());
                        app.add_log("Loading proving/verifying keys in background...".to_string());
                        app.status = "Select number of spots (1-8):".to_string();
                    }
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    #[cfg(feature = "wallet")]
                    if app.selected_mode == Some(GameMode::Contract) && matches!(app.phase, GamePhase::WaitingForReveal | GamePhase::PlayerTurn | GamePhase::DealerTurn) {
                        app.spawn_claim_timeout();
                    }
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if matches!(app.phase, GamePhase::ModeSelection | GamePhase::GameOver) {
                        #[cfg(feature = "wallet")]
                        {
                            app.selected_mode = Some(GameMode::Contract);
                            app.phase = GamePhase::ContractSetup;
                            app.add_log("CONTRACT mode selected".to_string());
                            if app.wallet.is_none() {
                                if let Ok(mnemonic) = std::env::var("PLAYER_MNEMONIC") {
                                    app.add_log("Loading wallet from PLAYER_MNEMONIC env...".to_string());
                                    if app.load_wallet_from_mnemonic(&mnemonic) {
                                        app.spawn_wallet_connect();
                                    }
                                } else {
                                    app.add_log("Enter mnemonic or press [G] to generate new wallet".to_string());
                                    app.status = "Enter mnemonic (press Enter to submit)".to_string();
                                    app.input_mode = InputMode::Mnemonic;
                                }
                            } else {
                                if app.contract_address.is_some() {
                                    app.add_log("Press [L] to list games, or [J] to join after selecting".to_string());
                                    app.status = "Press [L] to list available games".to_string();
                                } else {
                                    app.add_log("Enter contract address".to_string());
                                    app.status = "Enter contract address".to_string();
                                    app.input_mode = InputMode::ContractAddress;
                                }
                            }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            app.add_log("CONTRACT mode requires wallet feature".to_string());
                            app.add_log("Build with --features wallet to enable".to_string());
                        }
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_double_down();
                        } else {
                            // Local mode
                            let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                            if can_double {
                                let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    match game.double_down() {
                                        Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                        Err(e) => {
                                            app.add_log(format!("Error: {e}"));
                                            (spot + 1, hand + 1, num_hands, false, 0)
                                        }
                                    }
                                } else {
                                    (0, 0, 1, false, 0)
                                };

                                if success {
                                    let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                    app.add_log(format!("{hand_label} doubles down!"));
                                    if player_value > 21 { app.add_log(format!("{hand_label} busts with {player_value}!")); }
                                    if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                }
                            } else {
                                app.add_log("Cannot double down now".to_string());
                            }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                            if can_double {
                                let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    match game.double_down() {
                                        Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                        Err(e) => { app.add_log(format!("Error: {e}")); (spot + 1, hand + 1, num_hands, false, 0) }
                                    }
                                } else { (0, 0, 1, false, 0) };
                                if success {
                                    let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                    app.add_log(format!("{hand_label} doubles down!"));
                                    if player_value > 21 { app.add_log(format!("{hand_label} busts with {player_value}!")); }
                                    if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot double down now".to_string()); }
                        }
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_split();
                        } else {
                            let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                                if game.can_split() {
                                    let result = game.split();
                                    let spot = game.active_spot + 1;
                                    let can_double = game.can_double();
                                    (Some(result), spot, can_double)
                                } else { (None, 0, false) }
                            } else { (None, 0, false) };
                            if let Some(result) = split_result {
                                match result {
                                    Ok(_) => {
                                        app.add_log(format!("Spot {spot} splits!"));
                                        let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                        let mut options = vec!["[H]it", "[S]tand"];
                                        if can_double { options.push("[D]ouble"); }
                                        if can_surrender { options.push("Su[r]render"); }
                                        app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot split".to_string()); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                                if game.can_split() {
                                    let result = game.split();
                                    let spot = game.active_spot + 1;
                                    let can_double = game.can_double();
                                    (Some(result), spot, can_double)
                                } else { (None, 0, false) }
                            } else { (None, 0, false) };
                            if let Some(result) = split_result {
                                match result {
                                    Ok(_) => {
                                        app.add_log(format!("Spot {spot} splits!"));
                                        let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                        let mut options = vec!["[H]it", "[S]tand"];
                                        if can_double { options.push("[D]ouble"); }
                                        if can_surrender { options.push("Su[r]render"); }
                                        app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot split".to_string()); }
                        }
                    }
                }
                KeyCode::Char('g') | KeyCode::Char('G') => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup && app.wallet.is_none() {
                        match Wallet::generate("xion") {
                            Ok((wallet, mnemonic)) => {
                                app.add_log(format!("New wallet: {}", wallet.address()));
                                app.add_log(format!("Mnemonic: {mnemonic}"));
                                app.add_log("IMPORTANT: Save this mnemonic!".to_string());
                                app.wallet = Some(wallet);
                                app.input_mode = InputMode::Normal;

                                if let Ok(addr) = std::env::var("CONTRACT_ADDR") {
                                    app.contract_address = Some(addr.clone());
                                    app.add_log(format!("Contract: {addr}"));
                                    app.add_log("Press [L] to list games".to_string());
                                    app.status = "Press [L] to list available games".to_string();
                                } else {
                                    app.add_log("Enter contract address".to_string());
                                    app.status = "Enter contract address".to_string();
                                    app.input_mode = InputMode::ContractAddress;
                                }
                            }
                            Err(e) => {
                                app.add_log(format!("Wallet generation failed: {e}"));
                                app.status = "Wallet setup failed. Press [Q] to quit".to_string();
                            }
                        }
                    }
                }
                KeyCode::Char('0'..='9') => {
                    if app.phase == GamePhase::SpotSelection {
                        let num_spots = key.code.to_string().parse::<usize>().unwrap();
                        if (1..=8).contains(&num_spots) {
                            app.selected_spots = Some(num_spots);
                            app.phase = GamePhase::Initializing;
                            app.loading_dots = 0;
                            app.add_log(format!("Starting game with {num_spots} spot(s)"));
                        }
                    }
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup && !app.available_games.is_empty() {
                        let idx = key.code.to_string().parse::<usize>().unwrap();
                        if idx < app.available_games.len() {
                            let game_id = app.available_games[idx].game_id;
                            let dealer = app.available_games[idx].dealer.clone();
                            app.game_id = Some(game_id);
                            app.add_log(format!("Selected game {game_id} by dealer {dealer}"));
                            app.add_log("Press [J] to join this game".to_string());
                            app.status = format!("Press [J] to join game {game_id}");
                        }
                    }
                }
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_hit();
                        } else {
                            if let Err(e) = app.player_hit() {
                                app.add_log(format!("Error: {e}"));
                            }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            if let Err(e) = app.player_hit() {
                                app.add_log(format!("Error: {e}"));
                            }
                        }
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_stand();
                        } else {
                            if let Err(e) = app.player_stand() {
                                app.add_log(format!("Error: {e}"));
                            }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            if let Err(e) = app.player_stand() {
                                app.add_log(format!("Error: {e}"));
                            }
                        }
                    }
                }
                KeyCode::Char('j') | KeyCode::Char('J') => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup && app.wallet.is_some() && app.contract_address.is_some() && app.game_id.is_some() {
                        if app.wallet.as_ref().map_or(true, |w| w.client().is_none()) {
                            app.spawn_wallet_connect();
                            app.add_log("Connecting wallet first...".to_string());
                        } else {
                            app.spawn_join_game();
                        }
                    }
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_surrender();
                        } else {
                            let (surrender_result, spot, hand, num_hands) = if let Some(ref mut game) = app.game_state {
                                if game.can_surrender() {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    let result = game.surrender();
                                    (Some(result), spot + 1, hand + 1, num_hands)
                                } else { (None, 0, 0, 1) }
                            } else { (None, 0, 0, 1) };
                            if let Some(result) = surrender_result {
                                match result {
                                    Ok(_) => {
                                        let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                        app.add_log(format!("{hand_label} surrenders!"));
                                        if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot surrender".to_string()); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            let (surrender_result, spot, hand, num_hands) = if let Some(ref mut game) = app.game_state {
                                if game.can_surrender() {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    let result = game.surrender();
                                    (Some(result), spot + 1, hand + 1, num_hands)
                                } else { (None, 0, 0, 1) }
                            } else { (None, 0, 0, 1) };
                            if let Some(result) = surrender_result {
                                match result {
                                    Ok(_) => {
                                        let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                        app.add_log(format!("{hand_label} surrenders!"));
                                        if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot surrender".to_string()); }
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if app.phase == GamePhase::GameOver {
                        if let Some(task) = &mut app.next_game_task {
                            if task.is_finished() {
                                let task = app.next_game_task.take().unwrap();
                                match task.await {
                                    Ok(Ok(mut next_game)) => {
                                        app.add_log("--- New Game (pre-shuffled) ---".to_string());
                                        let num_spots = next_game.num_spots;
                                        for spot in 0..num_spots {
                                            if let Err(e) = next_game.draw_card(false, Some(spot)) { app.add_log(format!("Error dealing to spot {}: {}", spot + 1, e)); }
                                        }
                                        if let Err(e) = next_game.draw_card(true, None) { app.add_log(format!("Error dealing to dealer: {e}")); }
                                        for spot in 0..num_spots {
                                            if let Err(e) = next_game.draw_card(false, Some(spot)) { app.add_log(format!("Error dealing to spot {}: {}", spot + 1, e)); }
                                        }
                                        if let Err(e) = next_game.draw_card(true, None) { app.add_log(format!("Error dealing to dealer: {e}")); }

                                        app.game_state = Some(next_game);
                                        app.phase = GamePhase::PlayerTurn;
                                        app.spot_outcomes.clear();

                                        let should_peek = app.game_state.as_ref().map(|g| g.should_dealer_peek()).unwrap_or(false);
                                        if should_peek {
                                            if let Some(ref mut game) = app.game_state { game.dealer_peeked = true; }
                                            let has_blackjack = app.game_state.as_ref().map(|g| g.dealer_has_blackjack()).unwrap_or(false);
                                            if has_blackjack {
                                                app.add_log("Dealer peeks and has Blackjack!".to_string());
                                                if let Err(e) = app.dealer_play() { app.add_log(format!("Error: {e}")); }
                                                return Ok(());
                                            } else {
                                                app.add_log("Dealer peeks - no Blackjack".to_string());
                                            }
                                        }

                                        let game = app.game_state.as_ref().unwrap();
                                        let can_double = game.can_double();
                                        let can_split = game.can_split();
                                        let can_surrender = game.can_surrender();
                                        let mut options = vec!["[H]it", "[S]tand"];
                                        if can_double { options.push("[D]ouble"); }
                                        if can_split { options.push("S[p]lit"); }
                                        if can_surrender { options.push("Su[r]render"); }
                                        app.status = format!("Spot {}/{}: {}", game.active_spot + 1, game.num_spots, options.join(" or "));
                                        app.add_log(format!("Game started! Playing spot 1/{}", game.num_spots));

                                        let mode = app.selected_mode.unwrap();
                                        let num_spots = app.selected_spots.unwrap();
                                        app.add_log("Background: Pre-shuffling next game...".to_string());
                                        let next_task = tokio::task::spawn(async move {
                                            let mut next_game = GameState::new(mode, num_spots).await.map_err(|e| e.to_string())?;
                                            next_game.initialize_deck().map_err(|e| e.to_string())?;
                                            next_game.shuffle_deck().map_err(|e| e.to_string())?;
                                            log::info!("Background: Next game shuffled and ready!");
                                            Ok(next_game)
                                        });
                                        app.next_game_task = Some(next_task);
                                    }
                                    Ok(Err(e)) => {
                                        app.add_log(format!("Next game ERROR: {e}. Press [F] or [T] to restart"));
                                        app.status = "Next game failed. Press [F] or [T] to restart".to_string();
                                        app.phase = GamePhase::ModeSelection;
                                    }
                                    Err(e) => {
                                        app.add_log(format!("Next game task error: {e}. Press [F] or [T] to restart"));
                                        app.status = "Next game failed. Press [F] or [T] to restart".to_string();
                                        app.phase = GamePhase::ModeSelection;
                                    }
                                }
                            } else {
                                app.status = "Next game still shuffling...".to_string();
                            }
                        } else {
                            app.status = "No next game ready. Press [F] or [T] to restart".to_string();
                        }
                    }
                }
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup && app.wallet.is_some() && app.contract_address.is_some() {
                        if app.wallet.as_ref().map_or(true, |w| w.client().is_none()) {
                            app.spawn_wallet_connect();
                            app.add_log("Connecting wallet first...".to_string());
                        } else {
                            app.spawn_list_games();
                        }
                    } else {
                        app.log_visible = !app.log_visible;
                    }
                }
                KeyCode::Up => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_hit();
                        } else {
                            if let Err(e) = app.player_hit() { app.add_log(format!("Error: {e}")); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            if let Err(e) = app.player_hit() { app.add_log(format!("Error: {e}")); }
                        }
                    }
                }
                KeyCode::Down => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_stand();
                        } else {
                            if let Err(e) = app.player_stand() { app.add_log(format!("Error: {e}")); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            if let Err(e) = app.player_stand() { app.add_log(format!("Error: {e}")); }
                        }
                    }
                }
                KeyCode::Right => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_double_down();
                        } else {
                            let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                            if can_double {
                                let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    match game.double_down() {
                                        Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                        Err(e) => { app.add_log(format!("Error: {e}")); (spot + 1, hand + 1, num_hands, false, 0) }
                                    }
                                } else { (0, 0, 1, false, 0) };
                                if success {
                                    let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                    app.add_log(format!("{hand_label} doubles down!"));
                                    if player_value > 21 { app.add_log(format!("{hand_label} busts with {player_value}!")); }
                                    if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot double down now".to_string()); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                            if can_double {
                                let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                    let spot = game.active_spot;
                                    let hand = game.active_hand_in_spot;
                                    let num_hands = game.player_hands[spot].len();
                                    match game.double_down() {
                                        Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                        Err(e) => { app.add_log(format!("Error: {e}")); (spot + 1, hand + 1, num_hands, false, 0) }
                                    }
                                } else { (0, 0, 1, false, 0) };
                                if success {
                                    let hand_label = if num_hands > 1 { format!("Spot {spot}.{hand}") } else { format!("Spot {spot}") };
                                    app.add_log(format!("{hand_label} doubles down!"));
                                    if player_value > 21 { app.add_log(format!("{hand_label} busts with {player_value}!")); }
                                    if let Err(e) = app.move_to_next_spot_or_dealer() { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot double down now".to_string()); }
                        }
                    }
                }
                KeyCode::Left => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        #[cfg(feature = "wallet")]
                        if app.selected_mode == Some(GameMode::Contract) {
                            app.spawn_split();
                        } else {
                            let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                                if game.can_split() {
                                    let result = game.split();
                                    let spot = game.active_spot + 1;
                                    let can_double = game.can_double();
                                    (Some(result), spot, can_double)
                                } else { (None, 0, false) }
                            } else { (None, 0, false) };
                            if let Some(result) = split_result {
                                match result {
                                    Ok(_) => {
                                        app.add_log(format!("Spot {spot} splits!"));
                                        let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                        let mut options = vec!["[H]it", "[S]tand"];
                                        if can_double { options.push("[D]ouble"); }
                                        if can_surrender { options.push("Su[r]render"); }
                                        app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot split".to_string()); }
                        }
                        #[cfg(not(feature = "wallet"))]
                        {
                            let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                                if game.can_split() {
                                    let result = game.split();
                                    let spot = game.active_spot + 1;
                                    let can_double = game.can_double();
                                    (Some(result), spot, can_double)
                                } else { (None, 0, false) }
                            } else { (None, 0, false) };
                            if let Some(result) = split_result {
                                match result {
                                    Ok(_) => {
                                        app.add_log(format!("Spot {spot} splits!"));
                                        let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                        let mut options = vec!["[H]it", "[S]tand"];
                                        if can_double { options.push("[D]ouble"); }
                                        if can_surrender { options.push("Su[r]render"); }
                                        app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                    }
                                    Err(e) => { app.add_log(format!("Error: {e}")); }
                                }
                            } else { app.add_log("Cannot split".to_string()); }
                        }
                    }
                }
                KeyCode::Char(c) => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup {
                        if app.wallet.is_none() {
                            app.mnemonic_input.push(c);
                        } else if app.contract_address.is_none() {
                            app.contract_address_input.push(c);
                        }
                    }
                }
                KeyCode::Backspace => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup {
                        if app.wallet.is_none() {
                            app.mnemonic_input.pop();
                        } else if app.contract_address.is_none() {
                            app.contract_address_input.pop();
                        }
                    }
                }
                KeyCode::Enter => {
                    #[cfg(feature = "wallet")]
                    if app.phase == GamePhase::ContractSetup && app.wallet.is_none() && !app.mnemonic_input.is_empty() {
                        let mnemonic = app.mnemonic_input.clone();
                        app.mnemonic_input.clear();
                        if app.load_wallet_from_mnemonic(&mnemonic) {
                            app.spawn_wallet_connect();
                        }
                    } else if app.phase == GamePhase::ContractSetup && app.wallet.is_some() && app.contract_address.is_none() && !app.contract_address_input.is_empty() {
                        let addr = app.contract_address_input.clone();
                        app.contract_address = Some(addr.clone());
                        app.add_log(format!("Contract address set: {addr}"));
                        app.contract_address_input.clear();

                        if app.wallet.as_ref().map_or(true, |w| w.client().is_none()) {
                            app.spawn_wallet_connect();
                        } else {
                            app.spawn_list_games();
                        }
                    }
                }
                _ => {}
                } // close match key.code for Normal mode
                } // close match app.input_mode
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    // Main layout: Top section and bottom section
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),      // Title bar
                Constraint::Min(10),         // Main game area
                Constraint::Length(3),       // Status bar
            ]
            .as_ref(),
        )
        .split(f.area());

    // Title bar with game mode
    let title_text = if let Some(mode) = app.selected_mode {
        match mode {
            GameMode::Fast => "Juodžekas - Fast Mode (No Proofs)".to_string(),
            GameMode::Trustless => "Juodžekas - Trustless Mode (ZK Proofs)".to_string(),
            GameMode::Contract => "Juodžekas - Contract Mode (On-Chain)".to_string(),
        }
    } else {
        "Juodžekas - Trustless Blackjack".to_string()
    };

    let title = Paragraph::new(title_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, main_chunks[0]);

    // Split main area: left (game) and right (logs if visible)
    let (game_container, log_area) = if app.log_visible {
        let main_horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(main_chunks[1]);
        (main_horizontal[0], Some(main_horizontal[1]))
    } else {
        (main_chunks[1], None)
    };

    // Game area with dealer on top and player on bottom
    let game_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(game_container);

    // Dealer hand
    let dealer_cards: Vec<Span> = if let Some(ref game) = app.game_state {
        game.dealer_hand
            .iter()
            .enumerate()
            .map(|(idx, card_opt)| {
                // Hide dealer's second card until dealer's turn
                let card_str = if matches!(app.phase, GamePhase::PlayerTurn | GamePhase::Initializing) && idx == 1 {
                    "??".to_string()
                } else if let Some(card) = card_opt {
                    card.to_display()
                } else {
                    "??".to_string()
                };

                let color = match card_str.chars().last() {
                    Some('♥') => Color::Red,
                    Some('♦') => Color::from_u32(0xFF_A5_00), // Orange
                    Some('♣') => Color::Magenta, // Purple
                    Some('♠') => Color::Black,
                    _ => Color::White,
                };
                Span::styled(format!("{card_str} "), Style::default().fg(color).bg(Color::Gray))
            })
            .collect()
    } else if let Some(ref contract_game) = app.contract_game_state {
        contract_game.dealer_hand
            .iter()
            .map(|&card_idx| {
                let card = blackjack::Card::from_index(card_idx as usize);
                let card_str = card.to_display();
                let color = match card_str.chars().last() {
                    Some('♥') => Color::Red,
                    Some('♦') => Color::from_u32(0xFF_A5_00),
                    Some('♣') => Color::Magenta,
                    Some('♠') => Color::Black,
                    _ => Color::White,
                };
                Span::styled(format!("{card_str} "), Style::default().fg(color).bg(Color::Gray))
            })
            .collect()
    } else {
        vec![Span::raw("No game started")]
    };

    let dealer_value = if let Some(ref game) = app.game_state {
        if matches!(app.phase, GamePhase::DealerTurn | GamePhase::GameOver) {
            format!(" ({})", GameState::calculate_hand_value(&game.dealer_hand))
        } else {
            String::new()
        }
    } else if let Some(ref contract_game) = app.contract_game_state {
        if matches!(app.phase, GamePhase::DealerTurn | GamePhase::GameOver) {
            let value = calculate_hand_value_from_indices(&contract_game.dealer_hand);
            format!(" ({value})")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Calculate vertical centering for dealer hand
    let dealer_block_height = game_area[0].height.saturating_sub(2); // Subtract borders

    // Add arrow key instructions during player turn
    let mut dealer_lines: Vec<Line> = Vec::new();
    if matches!(app.phase, GamePhase::PlayerTurn) {
        // Build instructions based on available options
        let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
        let can_split = app.game_state.as_ref().map(|g| g.can_split()).unwrap_or(false);
        let optimal_move = app.game_state.as_ref().map(|g| g.get_optimal_move()).unwrap_or("Stand");

        let hit_style = if optimal_move == "Hit" {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow)
        };

        let stand_style = if optimal_move == "Stand" {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Yellow)
        };

        let mut instruction_spans = vec![
            Span::styled("↑", hit_style),
            Span::raw(" Hit  "),
            Span::styled("↓", stand_style),
            Span::raw(" Stand"),
        ];

        if can_double {
            let double_style = if optimal_move == "Double" {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            instruction_spans.push(Span::raw("  "));
            instruction_spans.push(Span::styled("→", double_style));
            instruction_spans.push(Span::raw(" Double"));
        }

        if can_split {
            let split_style = if optimal_move == "Split" {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            instruction_spans.push(Span::raw("  "));
            instruction_spans.push(Span::styled("←", split_style));
            instruction_spans.push(Span::raw(" Split"));
        }

        let instructions = vec![Line::from(instruction_spans)];
        let content_height = dealer_cards.len() + instructions.len() + 1; // cards + instructions + spacing
        let padding_top = dealer_block_height.saturating_sub(content_height as u16) / 2;

        dealer_lines.extend(vec![Line::from(""); padding_top as usize]);
        dealer_lines.push(Line::from(dealer_cards));
        dealer_lines.push(Line::from("")); // Spacing
        dealer_lines.extend(instructions);
    } else {
        let padding_lines = dealer_block_height / 2;
        dealer_lines.extend(vec![Line::from(""); padding_lines as usize]);
        dealer_lines.push(Line::from(dealer_cards));
    }

    let dealer_block = Paragraph::new(dealer_lines)
        .block(Block::default().title(format!(" Dealer Hand{dealer_value} ")).borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(dealer_block, game_area[0]); // Dealer on top

    // Player spots (horizontal layout)
    if let Some(ref game) = app.game_state {
        // Create horizontal layout for player spots
        let num_spots = game.num_spots;
        let spot_constraints: Vec<Constraint> = vec![Constraint::Ratio(1, num_spots as u32); num_spots];

        let spot_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(spot_constraints)
            .split(game_area[1]);

        // Render each spot
        for (i, spot_hands) in game.player_hands.iter().enumerate() {
            let num_hands_in_spot = spot_hands.len();

            // If spot is split, subdivide horizontally
            if num_hands_in_spot > 1 {
                let hand_constraints: Vec<Constraint> = vec![Constraint::Ratio(1, num_hands_in_spot as u32); num_hands_in_spot];
                let hand_areas = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(hand_constraints)
                    .split(spot_areas[i]);

                // Render each hand within the split spot
                for (j, hand) in spot_hands.iter().enumerate() {
                    let player_cards: Vec<Span> = hand
                        .iter()
                        .map(|card_opt| {
                            let card_str = if let Some(card) = card_opt {
                                card.to_display()
                            } else {
                                "??".to_string()
                            };

                            let color = match card_str.chars().last() {
                                Some('♥') => Color::Red,
                                Some('♦') => Color::from_u32(0xFF_A5_00), // Orange
                                Some('♣') => Color::Magenta, // Purple
                                Some('♠') => Color::Black,
                                _ => Color::White,
                            };
                            Span::styled(format!("{card_str} "), Style::default().fg(color).bg(Color::Gray))
                        })
                        .collect();

                    let player_value = GameState::calculate_hand_value(hand);

                    // Highlight active hand within split spot during play, or outcome at game over
                    let border_style = if i == game.active_spot && j == game.active_hand_in_spot && matches!(app.phase, GamePhase::PlayerTurn) {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else if app.phase == GamePhase::GameOver && i < app.spot_outcomes.len() && j < app.spot_outcomes[i].len() {
                        match app.spot_outcomes[i][j] {
                            SpotOutcome::Win => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            SpotOutcome::Loss => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            SpotOutcome::Push => Style::default().fg(Color::DarkGray),
                            SpotOutcome::Surrender => Style::default().fg(Color::from_u32(0xFF_A5_00)), // Orange
                        }
                    } else {
                        Style::default()
                    };

                    // Calculate wrapping based on available width
                    let hand_width = hand_areas[j].width.saturating_sub(2); // Subtract borders
                    let card_width = 4; // Each card is roughly "Ah♥ " = 4 chars
                    let cards_per_line = (hand_width / card_width).max(1) as usize;

                    // Wrap cards into multiple lines if needed
                    let mut wrapped_lines: Vec<Line> = Vec::new();
                    for chunk in player_cards.chunks(cards_per_line) {
                        wrapped_lines.push(Line::from(chunk.to_vec()));
                    }

                    // Calculate vertical centering
                    let hand_block_height = hand_areas[j].height.saturating_sub(2);
                    let content_lines = wrapped_lines.len();
                    let padding_top = (hand_block_height.saturating_sub(content_lines as u16)) / 2;

                    let mut hand_lines: Vec<Line> = vec![Line::from(""); padding_top as usize];
                    hand_lines.extend(wrapped_lines);

                    let hand_block = Paragraph::new(hand_lines)
                        .block(Block::default()
                            .title(format!(" {}.{} ({}) ", i + 1, j + 1, player_value))
                            .borders(Borders::ALL)
                            .border_style(border_style))
                        .alignment(Alignment::Center);
                    f.render_widget(hand_block, hand_areas[j]);
                }
            } else {
                // Single hand (not split)
                let hand = &spot_hands[0];
                let player_cards: Vec<Span> = hand
                    .iter()
                    .map(|card_opt| {
                        let card_str = if let Some(card) = card_opt {
                            card.to_display()
                        } else {
                            "??".to_string()
                        };

                        let color = match card_str.chars().last() {
                            Some('♥') => Color::Red,
                            Some('♦') => Color::from_u32(0xFF_A5_00), // Orange
                            Some('♣') => Color::Magenta, // Purple
                            Some('♠') => Color::Black,
                            _ => Color::White,
                        };
                        Span::styled(format!("{card_str} "), Style::default().fg(color).bg(Color::Gray))
                    })
                    .collect();

                let player_value = GameState::calculate_hand_value(hand);

                // Highlight active spot during play, or outcome at game over
                let border_style = if i == game.active_spot && matches!(app.phase, GamePhase::PlayerTurn) {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if app.phase == GamePhase::GameOver && i < app.spot_outcomes.len() && !app.spot_outcomes[i].is_empty() {
                    match app.spot_outcomes[i][0] {
                        SpotOutcome::Win => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        SpotOutcome::Loss => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        SpotOutcome::Push => Style::default().fg(Color::DarkGray),
                        SpotOutcome::Surrender => Style::default().fg(Color::from_u32(0xFF_A5_00)), // Orange
                    }
                } else {
                    Style::default()
                };

                // Calculate wrapping based on available width
                let spot_width = spot_areas[i].width.saturating_sub(2); // Subtract borders
                let card_width = 4; // Each card is roughly "Ah♥ " = 4 chars
                let cards_per_line = (spot_width / card_width).max(1) as usize;

                // Wrap cards into multiple lines if needed
                let mut wrapped_lines: Vec<Line> = Vec::new();
                for chunk in player_cards.chunks(cards_per_line) {
                    wrapped_lines.push(Line::from(chunk.to_vec()));
                }

                // Calculate vertical centering
                let spot_block_height = spot_areas[i].height.saturating_sub(2);
                let content_lines = wrapped_lines.len();
                let padding_top = (spot_block_height.saturating_sub(content_lines as u16)) / 2;

                let mut spot_lines: Vec<Line> = vec![Line::from(""); padding_top as usize];
                spot_lines.extend(wrapped_lines);

                let spot_block = Paragraph::new(spot_lines)
                    .block(Block::default()
                        .title(format!(" {} ({}) ", i + 1, player_value))
                        .borders(Borders::ALL)
                        .border_style(border_style))
                    .alignment(Alignment::Center);
                f.render_widget(spot_block, spot_areas[i]);
            }
        }
    } else if let Some(ref contract_game) = app.contract_game_state {
        // Contract mode: simplified rendering with card indices
        let num_hands = contract_game.hands.len();
        let hand_constraints: Vec<Constraint> = vec![Constraint::Ratio(1, num_hands as u32); num_hands];

        let hand_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(hand_constraints)
            .split(game_area[1]);

        for (i, hand) in contract_game.hands.iter().enumerate() {
            let player_cards: Vec<Span> = hand.cards
                .iter()
                .map(|&card_idx| {
                    let card = blackjack::Card::from_index(card_idx as usize);
                    let card_str = card.to_display();
                    let color = match card_str.chars().last() {
                        Some('♥') => Color::Red,
                        Some('♦') => Color::from_u32(0xFF_A5_00),
                        Some('♣') => Color::Magenta,
                        Some('♠') => Color::Black,
                        _ => Color::White,
                    };
                    Span::styled(format!("{card_str} "), Style::default().fg(color).bg(Color::Gray))
                })
                .collect();

            let player_value = calculate_hand_value_from_indices(&hand.cards);

            let hand_lines = vec![Line::from(player_cards)];

            let hand_block = Paragraph::new(hand_lines)
                .block(Block::default()
                    .title(format!(" Hand {} ({}) ", i + 1, player_value))
                    .borders(Borders::ALL))
                .alignment(Alignment::Center);
            f.render_widget(hand_block, hand_areas[i]);
        }
    } else {
        // No game started
        let no_game_block = Paragraph::new("No game started")
            .block(Block::default().title(" Player Spots ").borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(no_game_block, game_area[1]);
    }

    // Logs/Info box - only render if visible
    if let Some(log_area) = log_area {
        // Build log title with balance if wallet exists
        #[cfg(feature = "wallet")]
        let log_title = if let Some(ref balance) = app.wallet_balance {
            format!(" Game Log | Balance: {balance} ")
        } else {
            " Game Log ".to_string()
        };

        #[cfg(not(feature = "wallet"))]
        let log_title = " Game Log ".to_string();

        // Estimate how many log entries will fit (accounting for wrapping)
        let log_frame_height = log_area.height.saturating_sub(2) as usize; // Subtract borders
        let log_width = log_area.width.saturating_sub(4) as usize; // Subtract borders and bullet

        // Estimate wrapped lines for each log entry and collect enough to fill screen
        let mut total_wrapped_lines = 0;
        let mut logs_to_show = Vec::new();

        // Work backwards from the end to collect enough logs to fill the screen
        for log in app.logs.iter().rev() {
            let log_len = log.len() + 2; // Add bullet and space
            let wrapped_lines = (log_len / log_width.max(1)) + 1;

            if total_wrapped_lines + wrapped_lines <= log_frame_height || logs_to_show.is_empty() {
                logs_to_show.push(log);
                total_wrapped_lines += wrapped_lines;
            } else {
                break;
            }
        }

        logs_to_show.reverse();

        let log_lines: Vec<Line> = logs_to_show
            .iter()
            .map(|log| {
                Line::from(vec![
                    Span::styled("• ", Style::default().fg(Color::DarkGray)),
                    Span::raw((*log).clone()),
                ])
            })
            .collect();

        let logs_widget = Paragraph::new(log_lines)
            .block(
                Block::default()
                    .title(log_title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: true });
        f.render_widget(logs_widget, log_area);
    }

    // Status bar at bottom
    #[cfg(feature = "wallet")]
    let status_text = match app.input_mode {
        InputMode::Mnemonic => {
            if !app.mnemonic_input.is_empty() {
                format!("Mnemonic > {}", app.mnemonic_input)
            } else {
                "Enter mnemonic (or press [G] to generate): ".to_string()
            }
        }
        InputMode::ContractAddress => {
            if !app.contract_address_input.is_empty() {
                format!("Contract address > {}", app.contract_address_input)
            } else {
                "Enter contract address: ".to_string()
            }
        }
        InputMode::Normal => app.status.clone(),
    };

    #[cfg(not(feature = "wallet"))]
    let status_text = app.status.clone();

    let status_bar = Paragraph::new(status_text.as_str())
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(status_bar, main_chunks[2]);

    // Render help modal if visible
    if app.help_visible {
        render_help_modal(f);
    }
}

fn render_help_modal(f: &mut Frame) {
    use ratatui::widgets::Clear;

    // Center the help modal - 80% width, 80% height
    let area = f.area();
    let modal_width = (area.width * 80) / 100;
    let modal_height = (area.height * 80) / 100;
    let modal_x = (area.width - modal_width) / 2;
    let modal_y = (area.height - modal_height) / 2;

    let modal_area = ratatui::layout::Rect {
        x: modal_x,
        y: modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Clear the area before rendering modal
    f.render_widget(Clear, modal_area);

    // Render modal background
    let clear_block = Block::default()
        .style(Style::default().bg(Color::Black));
    f.render_widget(clear_block, modal_area);

    let help_text = vec![
        Line::from(vec![
            Span::styled("Juodžekas Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Game Modes:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  [F] - Fast Mode: Instant gameplay, no ZK proofs"),
        Line::from("  [T] - Trustless Mode: Full ZK proofs (~1 min setup)"),
        Line::from("  [C] - Contract Mode: On-chain with smart contract"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Gameplay Keys:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  [H] - Hit (draw a card)"),
        Line::from("  [S] - Stand (end turn)"),
        Line::from("  [D] - Double Down (double bet, draw one card, auto-stand)"),
        Line::from("  [P] - Split (split pair into two hands)"),
        Line::from("  [R] - Surrender (forfeit half bet, end hand)"),
        Line::from("  [N] - New game (after game ends)"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Contract Mode Keys:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  [G] - Generate new wallet"),
        Line::from("  [J] - Join selected game"),
        Line::from("  [X] - Claim timeout (if opponent doesn't respond)"),
        Line::from("  [0-9] - Select game from list"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Other Keys:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  [L] - Toggle log visibility"),
        Line::from("  [?] - Show/hide this help"),
        Line::from("  [Q] - Quit"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Spot Selection:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from("  [1-8] - Select number of spots to play"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press [?] to close", Style::default().fg(Color::Green).add_modifier(Modifier::ITALIC)),
        ]),
    ];

    let help_paragraph = Paragraph::new(help_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Help ")
                .title_alignment(Alignment::Center)
        )
        .wrap(Wrap { trim: true });

    f.render_widget(help_paragraph, modal_area);
}
