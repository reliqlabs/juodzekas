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

mod game;
use game::{GameState, GameMode};

mod tui_logger;
use tui_logger::TuiLogger;

#[derive(PartialEq)]
enum GamePhase {
    ModeSelection,
    SpotSelection,
    Initializing,
    PlayerTurn,
    DealerTurn,
    GameOver,
}

#[derive(Clone, Copy, PartialEq)]
enum SpotOutcome {
    Win,
    Loss,
    Push,
    Surrender,
}

struct App {
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
}

impl App {
    fn new(log_buffer: Arc<Mutex<Vec<String>>>) -> App {
        App {
            game_state: None,
            phase: GamePhase::ModeSelection,
            selected_mode: None,
            selected_spots: None,
            status: "Select mode: [F]ast (instant) or [T]rustless (~1 min, with ZK proofs)".to_string(),
            logs: vec![
                "Welcome to Juodžekas!".to_string(),
                "Choose your game mode:".to_string(),
                "[F] Fast - Instant gameplay, no proofs".to_string(),
                "[T] Trustless - Full ZK proofs, ~1 min setup".to_string(),
            ],
            log_buffer,
            loading_dots: 0,
            init_task: None,
            init_start_time: None,
            current_init_stage: String::new(),
            next_game_task: None,
            spot_outcomes: Vec::new(),
            log_visible: true,
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
            format!("Spot {}.{}", spot, hand_in_spot)
        } else {
            format!("Spot {}", spot)
        };

        self.add_log(format!("{} hits", hand_label));

        if player_value > 21 {
            self.add_log(format!("{} busts with {}!", hand_label, player_value));
            self.move_to_next_spot_or_dealer()?;
        } else if player_value == 21 {
            self.add_log(format!("{} has 21!", hand_label));
            self.move_to_next_spot_or_dealer()?;
        } else {
            self.add_log(format!("{} has {}", hand_label, player_value));
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
            self.add_log(format!("{} stands", hand_label));
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
                self.add_log(format!("{} has 21!", hand_label));
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
                self.add_log(format!("Playing {}", hand_label));
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

            self.add_log(format!("Dealer hits, has {}", dealer_value));

            if dealer_value > 21 {
                self.add_log(format!("Dealer busts with {}!", dealer_value));

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
                        self.add_log(format!("{}: Surrendered (half loss)", hand_label));
                        surrenders += 1;
                        SpotOutcome::Surrender
                    } else if player_value > 21 {
                        self.add_log(format!("{}: Bust (loss)", hand_label));
                        losses += 1;
                        SpotOutcome::Loss
                    } else {
                        self.add_log(format!("{}: {} - WIN", hand_label, player_value));
                        wins += 1;
                        SpotOutcome::Win
                    };
                    self.spot_outcomes[spot_idx][hand_idx] = outcome;
                }

                let status_msg = if surrenders > 0 {
                    format!("Dealer busts! {} wins, {} losses, {} surrenders. Press [N] for next game", wins, losses, surrenders)
                } else {
                    format!("Dealer busts! {} wins, {} losses. Press [N] for next game", wins, losses)
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

        self.add_log(format!("Dealer stands with {}", dealer_value));

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
                self.add_log(format!("{}: Surrendered (half loss)", hand_label));
                surrenders += 1;
                SpotOutcome::Surrender
            } else if player_value > 21 {
                self.add_log(format!("{}: Bust (loss)", hand_label));
                losses += 1;
                SpotOutcome::Loss
            } else if player_value > dealer_value {
                self.add_log(format!("{}: {} vs {} - WIN", hand_label, player_value, dealer_value));
                wins += 1;
                SpotOutcome::Win
            } else if dealer_value > player_value {
                self.add_log(format!("{}: {} vs {} - Loss", hand_label, player_value, dealer_value));
                losses += 1;
                SpotOutcome::Loss
            } else {
                self.add_log(format!("{}: {} vs {} - Push", hand_label, player_value, dealer_value));
                pushes += 1;
                SpotOutcome::Push
            };
            self.spot_outcomes[spot_idx][hand_idx] = outcome;
        }

        let status_msg = if surrenders > 0 {
            format!("Results: {} wins, {} losses, {} pushes, {} surrenders. Press [N] for next game", wins, losses, pushes, surrenders)
        } else {
            format!("Results: {} wins, {} losses, {} pushes. Press [N] for next game", wins, losses, pushes)
        };
        self.status = status_msg;

        self.phase = GamePhase::GameOver;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
        println!("{:?}", err)
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

        // Update loading animation and check logs for stage changes
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
                } else if last_log.contains("Proving key loaded") || last_log.contains("Initializing deck") {
                    Some("Shuffling")
                } else if last_log.contains("Shuffling deck") {
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
                        // Log completion time of previous stage
                        if !app.current_init_stage.is_empty() {
                            if let Some(start_time) = app.init_start_time {
                                let elapsed = start_time.elapsed().as_secs();
                                app.add_log(format!(">>> {} completed in {}s <<<", app.current_init_stage, elapsed));
                            }
                        }

                        app.current_init_stage = stage.to_string();
                        app.init_start_time = Some(std::time::Instant::now());
                        app.add_log(format!(">>> {} started <<<", stage));
                    }
                }
            }

            if let Some(start_time) = app.init_start_time {
                let elapsed = start_time.elapsed().as_secs();
                if !app.current_init_stage.is_empty() {
                    app.status = format!("{} {}s{:<3}", app.current_init_stage, elapsed, dots);
                } else if app.selected_mode == Some(GameMode::Trustless) {
                    app.status = format!("Initializing{}s{:<3}", elapsed, dots);
                } else if app.selected_mode == Some(GameMode::Fast) {
                    app.status = format!("Creating fast game{:<3}", dots);
                }
            }
        }

        terminal.draw(|f| ui(f, &app))?;

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

                            // Resize for actual number of spots
                            if let Err(e) = game_state.resize_for_spots(num_spots) {
                                app.add_log(format!("ERROR: {}", e));
                                app.status = "Error setting spots. Press [F] or [T] to try again".to_string();
                                app.phase = GamePhase::ModeSelection;
                                app.selected_mode = None;
                                app.selected_spots = None;
                                app.init_start_time = None;
                                app.current_init_stage.clear();
                            } else {
                                // Deal initial cards
                                for spot in 0..num_spots {
                                    if let Err(e) = game_state.draw_card(false, Some(spot)) {
                                        app.add_log(format!("Error dealing: {}", e));
                                    }
                                }
                                if let Err(e) = game_state.draw_card(true, None) {
                                    app.add_log(format!("Error dealing: {}", e));
                                }
                                for spot in 0..num_spots {
                                    if let Err(e) = game_state.draw_card(false, Some(spot)) {
                                        app.add_log(format!("Error dealing: {}", e));
                                    }
                                }
                                if let Err(e) = game_state.draw_card(true, None) {
                                    app.add_log(format!("Error dealing: {}", e));
                                }

                                app.game_state = Some(game_state);
                                app.phase = GamePhase::PlayerTurn;
                                app.init_start_time = None;
                                app.current_init_stage.clear();

                                // Check if dealer should peek for blackjack
                                let should_peek = app.game_state.as_ref().map(|g| g.should_dealer_peek()).unwrap_or(false);
                                if should_peek {
                                    if let Some(ref mut game) = app.game_state {
                                        game.dealer_peeked = true;
                                    }
                                    let has_blackjack = app.game_state.as_ref().map(|g| g.dealer_has_blackjack()).unwrap_or(false);
                                    if has_blackjack {
                                        app.add_log("Dealer peeks and has Blackjack!".to_string());
                                        if let Err(e) = app.dealer_play() {
                                            app.add_log(format!("Error: {}", e));
                                        }
                                        // Early return - game over, dealer has blackjack
                                        return Ok(());
                                    } else {
                                        app.add_log("Dealer peeks - no Blackjack".to_string());
                                    }
                                }

                                // Check if first spot has 21 and auto-advance
                                let first_spot_value = {
                                    let game = app.game_state.as_ref().unwrap();
                                    GameState::calculate_hand_value(&game.player_hands[0][0])
                                };

                                if first_spot_value == 21 {
                                    app.add_log(format!("Game started! Spot 1/{} has Blackjack!", num_spots));
                                    if let Err(e) = app.move_to_next_spot_or_dealer() {
                                        app.add_log(format!("Error: {}", e));
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

                                // Start pre-shuffling next game
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
                            app.add_log(format!("ERROR: {}", e));
                            app.status = "Error starting game. Press [F] or [T] to try again".to_string();
                            app.phase = GamePhase::ModeSelection;
                            app.selected_mode = None;
                            app.selected_spots = None;
                            app.init_start_time = None;
                            app.current_init_stage.clear();
                        }
                        Err(e) => {
                            app.add_log(format!("Task error: {}", e));
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

        // This block is now handled above during spot selection → initialization transition

        // Don't auto-transition - wait for user to press 'N'

        // Use poll with timeout so UI can refresh even during long operations
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => {
                    // Cancel any pending tasks
                    if let Some(task) = app.init_task.take() {
                        task.abort();
                    }
                    if let Some(task) = app.next_game_task.take() {
                        task.abort();
                    }
                    return Ok(());
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    if matches!(app.phase, GamePhase::ModeSelection | GamePhase::GameOver) {
                        app.selected_mode = Some(GameMode::Fast);
                        app.phase = GamePhase::SpotSelection;
                        app.add_log("FAST mode selected".to_string());
                        app.status = "Select number of spots (1-8):".to_string();
                    }
                }
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    if matches!(app.phase, GamePhase::ModeSelection | GamePhase::GameOver) {
                        app.selected_mode = Some(GameMode::Trustless);
                        app.phase = GamePhase::SpotSelection;
                        app.add_log("TRUSTLESS mode selected".to_string());
                        app.status = "Select number of spots (1-8):".to_string();
                    }
                }
                KeyCode::Char('1'..='8') => {
                    if app.phase == GamePhase::SpotSelection {
                        let num_spots = key.code.to_string().parse::<usize>().unwrap();
                        app.selected_spots = Some(num_spots);
                        app.phase = GamePhase::Initializing;
                        app.loading_dots = 0;
                        app.add_log(format!("Starting game with {} spot(s)", num_spots));
                        if app.selected_mode == Some(GameMode::Trustless) {
                            app.add_log("Loading proving/verifying keys...".to_string());
                        }
                    }
                }
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        if let Err(e) = app.player_hit() {
                            app.add_log(format!("Error: {}", e));
                        }
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        if let Err(e) = app.player_stand() {
                            app.add_log(format!("Error: {}", e));
                        }
                    }
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        let (surrender_result, spot, hand, num_hands) = if let Some(ref mut game) = app.game_state {
                            if game.can_surrender() {
                                let spot = game.active_spot;
                                let hand = game.active_hand_in_spot;
                                let num_hands = game.player_hands[spot].len();
                                let result = game.surrender();
                                (Some(result), spot + 1, hand + 1, num_hands)
                            } else {
                                (None, 0, 0, 1)
                            }
                        } else {
                            (None, 0, 0, 1)
                        };

                        if let Some(result) = surrender_result {
                            match result {
                                Ok(_) => {
                                    let hand_label = if num_hands > 1 {
                                        format!("Spot {}.{}", spot, hand)
                                    } else {
                                        format!("Spot {}", spot)
                                    };
                                    app.add_log(format!("{} surrenders!", hand_label));
                                    if let Err(e) = app.move_to_next_spot_or_dealer() {
                                        app.add_log(format!("Error: {}", e));
                                    }
                                }
                                Err(e) => {
                                    app.add_log(format!("Error: {}", e));
                                }
                            }
                        } else {
                            app.add_log("Cannot surrender".to_string());
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if app.phase == GamePhase::GameOver {
                        // Check if next game is ready
                        if let Some(task) = &mut app.next_game_task {
                            if task.is_finished() {
                                let task = app.next_game_task.take().unwrap();
                                match task.await {
                                    Ok(Ok(mut next_game)) => {
                                        app.add_log("--- New Game (pre-shuffled) ---".to_string());

                                        let num_spots = next_game.num_spots;

                                        // Deal initial cards for the next game
                                        for spot in 0..num_spots {
                                            if let Err(e) = next_game.draw_card(false, Some(spot)) {
                                                app.add_log(format!("Error dealing to spot {}: {}", spot + 1, e));
                                            }
                                        }
                                        if let Err(e) = next_game.draw_card(true, None) {
                                            app.add_log(format!("Error dealing to dealer: {}", e));
                                        }
                                        for spot in 0..num_spots {
                                            if let Err(e) = next_game.draw_card(false, Some(spot)) {
                                                app.add_log(format!("Error dealing to spot {}: {}", spot + 1, e));
                                            }
                                        }
                                        if let Err(e) = next_game.draw_card(true, None) {
                                            app.add_log(format!("Error dealing to dealer: {}", e));
                                        }

                                        app.game_state = Some(next_game);
                                        app.phase = GamePhase::PlayerTurn;
                                        app.spot_outcomes.clear(); // Clear previous outcomes

                                        // Check if dealer should peek for blackjack
                                        let should_peek = app.game_state.as_ref().map(|g| g.should_dealer_peek()).unwrap_or(false);
                                        if should_peek {
                                            if let Some(ref mut game) = app.game_state {
                                                game.dealer_peeked = true;
                                            }
                                            let has_blackjack = app.game_state.as_ref().map(|g| g.dealer_has_blackjack()).unwrap_or(false);
                                            if has_blackjack {
                                                app.add_log("Dealer peeks and has Blackjack!".to_string());
                                                if let Err(e) = app.dealer_play() {
                                                    app.add_log(format!("Error: {}", e));
                                                }
                                                // Early return - game over, dealer has blackjack
                                                return Ok(());
                                            } else {
                                                app.add_log("Dealer peeks - no Blackjack".to_string());
                                            }
                                        }

                                        // Build status message with available options
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

                                        // Start pre-shuffling the NEXT game
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
                                    Ok(Err(e)) => {
                                        app.add_log(format!("Next game ERROR: {}. Press [F] or [T] to restart", e));
                                        app.status = "Next game failed. Press [F] or [T] to restart".to_string();
                                        app.phase = GamePhase::ModeSelection;
                                    }
                                    Err(e) => {
                                        app.add_log(format!("Next game task error: {}. Press [F] or [T] to restart", e));
                                        app.status = "Next game failed. Press [F] or [T] to restart".to_string();
                                        app.phase = GamePhase::ModeSelection;
                                    }
                                }
                            } else {
                                // Next game not ready yet
                                app.status = "Next game still shuffling...".to_string();
                            }
                        } else {
                            // No next game task exists
                            app.status = "No next game ready. Press [F] or [T] to restart".to_string();
                        }
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                        if can_double {
                            let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                let spot = game.active_spot;
                                let hand = game.active_hand_in_spot;
                                let num_hands = game.player_hands[spot].len();
                                match game.double_down() {
                                    Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                    Err(e) => {
                                        app.add_log(format!("Error: {}", e));
                                        (spot + 1, hand + 1, num_hands, false, 0)
                                    }
                                }
                            } else {
                                (0, 0, 1, false, 0)
                            };

                            if success {
                                let hand_label = if num_hands > 1 {
                                    format!("Spot {}.{}", spot, hand)
                                } else {
                                    format!("Spot {}", spot)
                                };
                                app.add_log(format!("{} doubles down!", hand_label));
                                if player_value > 21 {
                                    app.add_log(format!("{} busts with {}!", hand_label, player_value));
                                }
                                // Double down auto-stands, so move to next hand/spot
                                if let Err(e) = app.move_to_next_spot_or_dealer() {
                                    app.add_log(format!("Error: {}", e));
                                }
                            }
                        } else {
                            app.add_log("Cannot double down now".to_string());
                        }
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                            if game.can_split() {
                                let result = game.split();
                                let spot = game.active_spot + 1;
                                let can_double = game.can_double();
                                (Some(result), spot, can_double)
                            } else {
                                (None, 0, false)
                            }
                        } else {
                            (None, 0, false)
                        };

                        if let Some(result) = split_result {
                            match result {
                                Ok(_) => {
                                    app.add_log(format!("Spot {} splits!", spot));
                                    let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                    let mut options = vec!["[H]it", "[S]tand"];
                                    if can_double { options.push("[D]ouble"); }
                                    if can_surrender { options.push("Su[r]render"); }
                                    app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                }
                                Err(e) => {
                                    app.add_log(format!("Error: {}", e));
                                }
                            }
                        } else {
                            app.add_log("Cannot split".to_string());
                        }
                    }
                }
                KeyCode::Char('l') | KeyCode::Char('L') => {
                    app.log_visible = !app.log_visible;
                }
                KeyCode::Up => {
                    // Arrow Up = Hit
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        if let Err(e) = app.player_hit() {
                            app.add_log(format!("Error: {}", e));
                        }
                    }
                }
                KeyCode::Down => {
                    // Arrow Down = Stand
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        if let Err(e) = app.player_stand() {
                            app.add_log(format!("Error: {}", e));
                        }
                    }
                }
                KeyCode::Right => {
                    // Arrow Right = Double
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        let can_double = app.game_state.as_ref().map(|g| g.can_double()).unwrap_or(false);
                        if can_double {
                            let (spot, hand, num_hands, success, player_value) = if let Some(ref mut game) = app.game_state {
                                let spot = game.active_spot;
                                let hand = game.active_hand_in_spot;
                                let num_hands = game.player_hands[spot].len();
                                match game.double_down() {
                                    Ok(_) => (spot + 1, hand + 1, num_hands, true, GameState::calculate_hand_value(&game.player_hands[spot][hand])),
                                    Err(e) => {
                                        app.add_log(format!("Error: {}", e));
                                        (spot + 1, hand + 1, num_hands, false, 0)
                                    }
                                }
                            } else {
                                (0, 0, 1, false, 0)
                            };

                            if success {
                                let hand_label = if num_hands > 1 {
                                    format!("Spot {}.{}", spot, hand)
                                } else {
                                    format!("Spot {}", spot)
                                };
                                app.add_log(format!("{} doubles down!", hand_label));
                                if player_value > 21 {
                                    app.add_log(format!("{} busts with {}!", hand_label, player_value));
                                }
                                if let Err(e) = app.move_to_next_spot_or_dealer() {
                                    app.add_log(format!("Error: {}", e));
                                }
                            }
                        } else {
                            app.add_log("Cannot double down now".to_string());
                        }
                    }
                }
                KeyCode::Left => {
                    // Arrow Left = Split
                    if matches!(app.phase, GamePhase::PlayerTurn) {
                        let (split_result, spot, can_double) = if let Some(ref mut game) = app.game_state {
                            if game.can_split() {
                                let result = game.split();
                                let spot = game.active_spot + 1;
                                let can_double = game.can_double();
                                (Some(result), spot, can_double)
                            } else {
                                (None, 0, false)
                            }
                        } else {
                            (None, 0, false)
                        };

                        if let Some(result) = split_result {
                            match result {
                                Ok(_) => {
                                    app.add_log(format!("Spot {} splits!", spot));
                                    let can_surrender = app.game_state.as_ref().map(|g| g.can_surrender()).unwrap_or(false);
                                    let mut options = vec!["[H]it", "[S]tand"];
                                    if can_double { options.push("[D]ouble"); }
                                    if can_surrender { options.push("Su[r]render"); }
                                    app.status = format!("Spot {}.1/2: {}", spot, options.join(" or "));
                                }
                                Err(e) => {
                                    app.add_log(format!("Error: {}", e));
                                }
                            }
                        } else {
                            app.add_log("Cannot split".to_string());
                        }
                    }
                }
                _ => {}
            }
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
                Span::styled(format!("{} ", card_str), Style::default().fg(color).bg(Color::White))
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
        .block(Block::default().title(format!(" Dealer Hand{} ", dealer_value)).borders(Borders::ALL))
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
                            Span::styled(format!("{} ", card_str), Style::default().fg(color).bg(Color::White))
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
                        Span::styled(format!("{} ", card_str), Style::default().fg(color).bg(Color::White))
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
    } else {
        // No game started
        let no_game_block = Paragraph::new("No game started")
            .block(Block::default().title(" Player Spots ").borders(Borders::ALL))
            .alignment(Alignment::Center);
        f.render_widget(no_game_block, game_area[1]);
    }

    // Logs/Info box - only render if visible
    if let Some(log_area) = log_area {
        let log_frame_height = log_area.height.saturating_sub(2) as usize; // Subtract borders
        let log_start_idx = app.logs.len().saturating_sub(log_frame_height);

        let log_lines: Vec<Line> = app
            .logs
            .iter()
            .skip(log_start_idx)
            .map(|log| {
                Line::from(vec![
                    Span::styled("• ", Style::default().fg(Color::DarkGray)),
                    Span::raw(log.clone()),
                ])
            })
            .collect();

        let logs_widget = Paragraph::new(log_lines)
            .block(
                Block::default()
                    .title(" Game Log ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: true });
        f.render_widget(logs_widget, log_area);
    }

    // Status bar at bottom
    let status_bar = Paragraph::new(app.status.as_str())
        .style(Style::default().fg(Color::White))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(status_bar, main_chunks[2]);
}
