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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_hand_value_simple() {
        let cards = vec![Card::TwoHearts, Card::ThreeSpades];
        assert_eq!(calculate_hand_value(&cards), 5);
    }

    #[test]
    fn test_calculate_hand_value_with_face_cards() {
        let cards = vec![Card::KingHearts, Card::QueenSpades];
        assert_eq!(calculate_hand_value(&cards), 20);
    }

    #[test]
    fn test_calculate_hand_value_blackjack() {
        let cards = vec![Card::AceHearts, Card::KingSpades];
        assert_eq!(calculate_hand_value(&cards), 21);
    }

    #[test]
    fn test_calculate_hand_value_soft_ace() {
        let cards = vec![Card::AceHearts, Card::SixSpades];
        assert_eq!(calculate_hand_value(&cards), 17); // Ace as 11
    }

    #[test]
    fn test_calculate_hand_value_hard_ace() {
        let cards = vec![Card::AceHearts, Card::SixSpades, Card::NineClubs];
        assert_eq!(calculate_hand_value(&cards), 16); // Ace as 1
    }

    #[test]
    fn test_calculate_hand_value_multiple_aces() {
        let cards = vec![Card::AceHearts, Card::AceSpades, Card::NineClubs];
        assert_eq!(calculate_hand_value(&cards), 21); // One ace as 11, one as 1
    }

    #[test]
    fn test_is_busted() {
        let cards = vec![Card::KingHearts, Card::QueenSpades, Card::FiveClubs];
        assert!(is_busted(&cards));
    }

    #[test]
    fn test_not_busted() {
        let cards = vec![Card::KingHearts, Card::QueenSpades];
        assert!(!is_busted(&cards));
    }

    #[test]
    fn test_is_blackjack() {
        let cards = vec![Card::AceHearts, Card::KingSpades];
        assert!(is_blackjack(&cards));
    }

    #[test]
    fn test_not_blackjack_three_cards() {
        let cards = vec![Card::SevenHearts, Card::SevenSpades, Card::SevenClubs];
        assert!(!is_blackjack(&cards));
    }

    #[test]
    fn test_not_blackjack_wrong_value() {
        let cards = vec![Card::KingHearts, Card::QueenSpades];
        assert!(!is_blackjack(&cards));
    }

    #[test]
    fn test_is_soft_hand() {
        let cards = vec![Card::AceHearts, Card::SixSpades];
        assert!(is_soft_hand(&cards));
    }

    #[test]
    fn test_not_soft_hand_hard_ace() {
        let cards = vec![Card::AceHearts, Card::SixSpades, Card::NineClubs];
        assert!(!is_soft_hand(&cards));
    }

    #[test]
    fn test_not_soft_hand_no_ace() {
        let cards = vec![Card::KingHearts, Card::QueenSpades];
        assert!(!is_soft_hand(&cards));
    }

    #[test]
    fn test_can_split_cards_same_rank() {
        let card1 = Card::EightHearts;
        let card2 = Card::EightSpades;
        assert!(can_split_cards(&card1, &card2));
    }

    #[test]
    fn test_can_split_cards_different_rank() {
        let card1 = Card::EightHearts;
        let card2 = Card::NineSpades;
        assert!(!can_split_cards(&card1, &card2));
    }

    #[test]
    fn test_can_split_cards_face_cards() {
        let card1 = Card::KingHearts;
        let card2 = Card::QueenSpades;
        assert!(!can_split_cards(&card1, &card2)); // Different ranks
    }

    #[test]
    fn test_hand_struct_value() {
        let mut hand = Hand::new();
        hand.add_card(Card::KingHearts);
        hand.add_card(Card::SevenSpades);
        assert_eq!(hand.value(), 17);
    }

    #[test]
    fn test_hand_struct_is_blackjack() {
        let mut hand = Hand::new();
        hand.add_card(Card::AceHearts);
        hand.add_card(Card::KingSpades);
        assert!(hand.is_blackjack());
    }

    #[test]
    fn test_hand_struct_can_split() {
        let mut hand = Hand::new();
        hand.add_card(Card::EightHearts);
        hand.add_card(Card::EightSpades);
        assert!(hand.can_split());
    }

    #[test]
    fn test_hand_struct_cannot_split_three_cards() {
        let mut hand = Hand::new();
        hand.add_card(Card::EightHearts);
        hand.add_card(Card::EightSpades);
        hand.add_card(Card::TwoClubs);
        assert!(!hand.can_split());
    }
}
