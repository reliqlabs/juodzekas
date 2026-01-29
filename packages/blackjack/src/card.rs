use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Card {
    AceSpades, TwoSpades, ThreeSpades, FourSpades, FiveSpades, SixSpades, SevenSpades,
    EightSpades, NineSpades, TenSpades, JackSpades, QueenSpades, KingSpades,
    AceHearts, TwoHearts, ThreeHearts, FourHearts, FiveHearts, SixHearts, SevenHearts,
    EightHearts, NineHearts, TenHearts, JackHearts, QueenHearts, KingHearts,
    AceDiamonds, TwoDiamonds, ThreeDiamonds, FourDiamonds, FiveDiamonds, SixDiamonds, SevenDiamonds,
    EightDiamonds, NineDiamonds, TenDiamonds, JackDiamonds, QueenDiamonds, KingDiamonds,
    AceClubs, TwoClubs, ThreeClubs, FourClubs, FiveClubs, SixClubs, SevenClubs,
    EightClubs, NineClubs, TenClubs, JackClubs, QueenClubs, KingClubs,
}

impl Card {
    pub fn to_display(&self) -> String {
        match self {
            Card::AceSpades => "A♠".to_string(),
            Card::TwoSpades => "2♠".to_string(),
            Card::ThreeSpades => "3♠".to_string(),
            Card::FourSpades => "4♠".to_string(),
            Card::FiveSpades => "5♠".to_string(),
            Card::SixSpades => "6♠".to_string(),
            Card::SevenSpades => "7♠".to_string(),
            Card::EightSpades => "8♠".to_string(),
            Card::NineSpades => "9♠".to_string(),
            Card::TenSpades => "10♠".to_string(),
            Card::JackSpades => "J♠".to_string(),
            Card::QueenSpades => "Q♠".to_string(),
            Card::KingSpades => "K♠".to_string(),
            Card::AceHearts => "A♥".to_string(),
            Card::TwoHearts => "2♥".to_string(),
            Card::ThreeHearts => "3♥".to_string(),
            Card::FourHearts => "4♥".to_string(),
            Card::FiveHearts => "5♥".to_string(),
            Card::SixHearts => "6♥".to_string(),
            Card::SevenHearts => "7♥".to_string(),
            Card::EightHearts => "8♥".to_string(),
            Card::NineHearts => "9♥".to_string(),
            Card::TenHearts => "10♥".to_string(),
            Card::JackHearts => "J♥".to_string(),
            Card::QueenHearts => "Q♥".to_string(),
            Card::KingHearts => "K♥".to_string(),
            Card::AceDiamonds => "A♦".to_string(),
            Card::TwoDiamonds => "2♦".to_string(),
            Card::ThreeDiamonds => "3♦".to_string(),
            Card::FourDiamonds => "4♦".to_string(),
            Card::FiveDiamonds => "5♦".to_string(),
            Card::SixDiamonds => "6♦".to_string(),
            Card::SevenDiamonds => "7♦".to_string(),
            Card::EightDiamonds => "8♦".to_string(),
            Card::NineDiamonds => "9♦".to_string(),
            Card::TenDiamonds => "10♦".to_string(),
            Card::JackDiamonds => "J♦".to_string(),
            Card::QueenDiamonds => "Q♦".to_string(),
            Card::KingDiamonds => "K♦".to_string(),
            Card::AceClubs => "A♣".to_string(),
            Card::TwoClubs => "2♣".to_string(),
            Card::ThreeClubs => "3♣".to_string(),
            Card::FourClubs => "4♣".to_string(),
            Card::FiveClubs => "5♣".to_string(),
            Card::SixClubs => "6♣".to_string(),
            Card::SevenClubs => "7♣".to_string(),
            Card::EightClubs => "8♣".to_string(),
            Card::NineClubs => "9♣".to_string(),
            Card::TenClubs => "10♣".to_string(),
            Card::JackClubs => "J♣".to_string(),
            Card::QueenClubs => "Q♣".to_string(),
            Card::KingClubs => "K♣".to_string(),
        }
    }

    pub fn value(&self) -> u8 {
        match self {
            Card::AceSpades | Card::AceHearts | Card::AceDiamonds | Card::AceClubs => 11,
            Card::TwoSpades | Card::TwoHearts | Card::TwoDiamonds | Card::TwoClubs => 2,
            Card::ThreeSpades | Card::ThreeHearts | Card::ThreeDiamonds | Card::ThreeClubs => 3,
            Card::FourSpades | Card::FourHearts | Card::FourDiamonds | Card::FourClubs => 4,
            Card::FiveSpades | Card::FiveHearts | Card::FiveDiamonds | Card::FiveClubs => 5,
            Card::SixSpades | Card::SixHearts | Card::SixDiamonds | Card::SixClubs => 6,
            Card::SevenSpades | Card::SevenHearts | Card::SevenDiamonds | Card::SevenClubs => 7,
            Card::EightSpades | Card::EightHearts | Card::EightDiamonds | Card::EightClubs => 8,
            Card::NineSpades | Card::NineHearts | Card::NineDiamonds | Card::NineClubs => 9,
            _ => 10, // Ten, Jack, Queen, King
        }
    }

    pub fn rank(&self) -> u8 {
        match self {
            Card::AceSpades | Card::AceHearts | Card::AceDiamonds | Card::AceClubs => 1,
            Card::TwoSpades | Card::TwoHearts | Card::TwoDiamonds | Card::TwoClubs => 2,
            Card::ThreeSpades | Card::ThreeHearts | Card::ThreeDiamonds | Card::ThreeClubs => 3,
            Card::FourSpades | Card::FourHearts | Card::FourDiamonds | Card::FourClubs => 4,
            Card::FiveSpades | Card::FiveHearts | Card::FiveDiamonds | Card::FiveClubs => 5,
            Card::SixSpades | Card::SixHearts | Card::SixDiamonds | Card::SixClubs => 6,
            Card::SevenSpades | Card::SevenHearts | Card::SevenDiamonds | Card::SevenClubs => 7,
            Card::EightSpades | Card::EightHearts | Card::EightDiamonds | Card::EightClubs => 8,
            Card::NineSpades | Card::NineHearts | Card::NineDiamonds | Card::NineClubs => 9,
            Card::TenSpades | Card::TenHearts | Card::TenDiamonds | Card::TenClubs => 10,
            Card::JackSpades | Card::JackHearts | Card::JackDiamonds | Card::JackClubs => 11,
            Card::QueenSpades | Card::QueenHearts | Card::QueenDiamonds | Card::QueenClubs => 12,
            Card::KingSpades | Card::KingHearts | Card::KingDiamonds | Card::KingClubs => 13,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Card::AceSpades, 1 => Card::TwoSpades, 2 => Card::ThreeSpades, 3 => Card::FourSpades,
            4 => Card::FiveSpades, 5 => Card::SixSpades, 6 => Card::SevenSpades, 7 => Card::EightSpades,
            8 => Card::NineSpades, 9 => Card::TenSpades, 10 => Card::JackSpades, 11 => Card::QueenSpades,
            12 => Card::KingSpades, 13 => Card::AceHearts, 14 => Card::TwoHearts, 15 => Card::ThreeHearts,
            16 => Card::FourHearts, 17 => Card::FiveHearts, 18 => Card::SixHearts, 19 => Card::SevenHearts,
            20 => Card::EightHearts, 21 => Card::NineHearts, 22 => Card::TenHearts, 23 => Card::JackHearts,
            24 => Card::QueenHearts, 25 => Card::KingHearts, 26 => Card::AceDiamonds, 27 => Card::TwoDiamonds,
            28 => Card::ThreeDiamonds, 29 => Card::FourDiamonds, 30 => Card::FiveDiamonds, 31 => Card::SixDiamonds,
            32 => Card::SevenDiamonds, 33 => Card::EightDiamonds, 34 => Card::NineDiamonds, 35 => Card::TenDiamonds,
            36 => Card::JackDiamonds, 37 => Card::QueenDiamonds, 38 => Card::KingDiamonds, 39 => Card::AceClubs,
            40 => Card::TwoClubs, 41 => Card::ThreeClubs, 42 => Card::FourClubs, 43 => Card::FiveClubs,
            44 => Card::SixClubs, 45 => Card::SevenClubs, 46 => Card::EightClubs, 47 => Card::NineClubs,
            48 => Card::TenClubs, 49 => Card::JackClubs, 50 => Card::QueenClubs, 51 => Card::KingClubs,
            _ => panic!("Invalid card index: {}", index),
        }
    }
}
