use crate::Card;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandOutcome {
    Win,
    Loss,
    Push,
    Surrender,
    Blackjack,
}

/// Calculate the value of a blackjack hand
pub fn calculate_hand_value(cards: &[Card]) -> u8 {
    let mut total = 0;
    let mut aces = 0;

    for card in cards {
        let value = card.value();
        if value == 11 {
            aces += 1;
        }
        total += value;
    }

    // Adjust for aces
    while total > 21 && aces > 0 {
        total -= 10; // Count ace as 1 instead of 11
        aces -= 1;
    }

    total
}

/// Check if a hand is soft (has an ace counted as 11)
pub fn is_soft_hand(cards: &[Card]) -> bool {
    let has_ace = cards.iter().any(|c| c.value() == 11);
    let value = calculate_hand_value(cards);
    has_ace && value <= 21 && cards.iter().filter(|c| c.value() == 11).count() > 0
        && cards.iter().map(|c| if c.value() == 11 { 1 } else { c.value() }).sum::<u8>() + 10 == value
}

/// Check if a hand is busted
pub fn is_busted(cards: &[Card]) -> bool {
    calculate_hand_value(cards) > 21
}

/// Check if a hand is blackjack (21 with 2 cards)
pub fn is_blackjack(cards: &[Card]) -> bool {
    cards.len() == 2 && calculate_hand_value(cards) == 21
}

/// Check if two cards can be split (same rank)
pub fn can_split_cards(card1: &Card, card2: &Card) -> bool {
    card1.rank() == card2.rank()
}

pub struct Hand {
    pub cards: Vec<Card>,
    pub doubled: bool,
    pub stood: bool,
    pub surrendered: bool,
}

impl Hand {
    pub fn new() -> Self {
        Self {
            cards: Vec::new(),
            doubled: false,
            stood: false,
            surrendered: false,
        }
    }

    pub fn value(&self) -> u8 {
        calculate_hand_value(&self.cards)
    }

    pub fn is_soft(&self) -> bool {
        is_soft_hand(&self.cards)
    }

    pub fn is_busted(&self) -> bool {
        is_busted(&self.cards)
    }

    pub fn is_blackjack(&self) -> bool {
        is_blackjack(&self.cards)
    }

    pub fn add_card(&mut self, card: Card) {
        self.cards.push(card);
    }

    pub fn can_split(&self) -> bool {
        self.cards.len() == 2 && can_split_cards(&self.cards[0], &self.cards[1])
    }
}

impl Default for Hand {
    fn default() -> Self {
        Self::new()
    }
}
