use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::{error::Error, io};

struct App {
    player_hand: Vec<String>,
    dealer_hand: Vec<String>,
    status: String,
}

impl App {
    fn new() -> App {
        App {
            player_hand: vec!["A♠".to_string(), "J♥".to_string()],
            dealer_hand: vec!["7♦".to_string(), "??".to_string()],
            status: "Your Turn: [H]it, [S]tand, [D]ouble, [P]lit, [R]urrender".to_string(),
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new();
    let res = run_app(&mut terminal, app);

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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<(), Box<dyn Error>>
where
    B::Error: 'static,
{
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('h') => app.status = "Action: Hit".to_string(),
                KeyCode::Char('s') => app.status = "Action: Stand".to_string(),
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.area());

    let title = Paragraph::new("Juodžekas - Trustless Blackjack")
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let game_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    let dealer_cards: Vec<Span> = app
        .dealer_hand
        .iter()
        .map(|card| {
            let color = match card.chars().last() {
                Some('♥') => Color::Red,
                Some('♦') => Color::from_u32(0xFF_A5_00), // Orange
                Some('♣') => Color::Magenta, // Purple
                Some('♠') => Color::Black,
                _ => Color::White,
            };
            Span::styled(format!("{} ", card), Style::default().fg(color))
        })
        .collect();

    let dealer_block = Paragraph::new(Line::from(dealer_cards))
        .block(Block::default().title(" Dealer Hand ").borders(Borders::ALL));
    f.render_widget(dealer_block, game_area[0]);

    let player_cards: Vec<Span> = app
        .player_hand
        .iter()
        .map(|card| {
            let color = match card.chars().last() {
                Some('♥') => Color::Red,
                Some('♦') => Color::from_u32(0xFF_A5_00), // Orange
                Some('♣') => Color::Magenta, // Purple
                Some('♠') => Color::Black,
                _ => Color::White,
            };
            Span::styled(format!("{} ", card), Style::default().fg(color))
        })
        .collect();

    let player_block = Paragraph::new(Line::from(player_cards))
        .block(Block::default().title(" Player Hand ").borders(Borders::ALL));
    f.render_widget(player_block, game_area[1]);

    let status_bar = Paragraph::new(app.status.as_str())
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(status_bar, chunks[2]);
}
