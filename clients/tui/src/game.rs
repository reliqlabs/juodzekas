use zk_shuffle::elgamal::{KeyPair, Ciphertext, encrypt};
use zk_shuffle::shuffle::shuffle;
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::babyjubjub::{Point, Fr};
use zk_shuffle::proof::{
    generate_shuffle_proof_rapidsnark, verify_shuffle_proof_rapidsnark,
};
use ark_std::UniformRand;
use ark_ec::{CurveGroup, AffineRepr};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GameMode {
    Fast,      // No ZK proofs, instant gameplay
    Trustless, // Full ZK proofs, takes ~3-4 minutes to start
}

#[derive(Debug, Clone, PartialEq)]
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

pub struct GameState {
    pub player_keys: KeyPair,
    pub dealer_keys: KeyPair,
    pub aggregated_pk: Point,
    pub card_mapping: Vec<Point>, // Maps card index to elliptic curve point
    pub encrypted_deck: Vec<Ciphertext>,
    pub player_hands: Vec<Vec<Vec<Option<Card>>>>, // Spots -> Hands within spot (for splits) -> Cards
    pub dealer_hand: Vec<Option<Card>>,
    pub num_spots: usize, // Number of active spots (1-8)
    pub deck_position: usize,
    pub rng: ChaCha8Rng,
    pub mode: GameMode,
    pub active_spot: usize, // Current spot being played (0-indexed)
    pub active_hand_in_spot: usize, // When spot is split, which hand within spot (0-indexed)
    pub hands_doubled: Vec<Vec<bool>>, // Track which hands have doubled [spot][hand_in_spot]
    pub hands_stood: Vec<Vec<bool>>, // Track which hands have stood [spot][hand_in_spot]
}

impl GameState {
    pub async fn new(mode: GameMode, num_spots: usize) -> Result<Self, Box<dyn std::error::Error>> {
        if num_spots == 0 || num_spots > 8 {
            return Err("Number of spots must be between 1 and 8".into());
        }

        Self::new_with_spots(mode, num_spots).await
    }

    pub async fn new_uninitialized(mode: GameMode) -> Result<Self, Box<dyn std::error::Error>> {
        // Create with 1 spot as placeholder - will be resized before dealing
        Self::new_with_spots(mode, 1).await
    }

    async fn new_with_spots(mode: GameMode, num_spots: usize) -> Result<Self, Box<dyn std::error::Error>> {

        let mut rng = ChaCha8Rng::from_entropy();

        // Generate keypairs for player and dealer
        let player_keys = KeyPair::generate(&mut rng);
        let dealer_keys = KeyPair::generate(&mut rng);

        // Aggregate public key
        let aggregated_pk = (player_keys.pk.into_group() + dealer_keys.pk.into_group()).into_affine();

        // Create card mapping (52 cards)
        let g = Point::generator();
        let mut card_mapping = Vec::new();
        for i in 1..=52 {
            let card_point = (g.into_group() * Fr::from(i as u64)).into_affine();
            card_mapping.push(card_point);
        }

        // Initialize empty hands for each spot - each spot starts with one hand (index 0)
        let player_hands = vec![vec![Vec::new()]; num_spots];
        let hands_doubled = vec![vec![false]; num_spots];
        let hands_stood = vec![vec![false]; num_spots];

        Ok(GameState {
            player_keys,
            dealer_keys,
            aggregated_pk,
            card_mapping,
            encrypted_deck: Vec::new(),
            player_hands,
            dealer_hand: Vec::new(),
            num_spots,
            deck_position: 0,
            rng,
            mode,
            active_spot: 0,
            active_hand_in_spot: 0,
            hands_doubled,
            hands_stood,
        })
    }

    // No longer needed - rapidsnark uses zkey files directly
    // Keys are loaded on-demand during proof generation

    pub fn initialize_deck(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Encrypt all cards with the aggregated public key
        let encrypted_deck: Vec<Ciphertext> = self.card_mapping.iter().map(|card_point| {
            let r = Fr::rand(&mut self.rng);
            encrypt(&self.aggregated_pk, card_point, &r)
        }).collect();

        self.encrypted_deck = encrypted_deck;
        self.deck_position = 0;
        Ok(())
    }

    pub fn shuffle_deck(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.mode == GameMode::Trustless {
            self.shuffle_deck_with_proofs()
        } else {
            self.shuffle_deck_fast()
        }
    }

    fn shuffle_deck_fast(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Player shuffles (no proofs)
        let player_shuffle = shuffle(&mut self.rng, &self.encrypted_deck, &self.aggregated_pk);
        self.encrypted_deck = player_shuffle.deck;

        // Dealer shuffles (no proofs)
        let dealer_shuffle = shuffle(&mut self.rng, &self.encrypted_deck, &self.aggregated_pk);
        self.encrypted_deck = dealer_shuffle.deck;

        Ok(())
    }

    fn shuffle_deck_with_proofs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let shuffle_vkey_path = "circuits/artifacts/shuffle_encrypt_vkey.json";

        log::info!("Generating player shuffle proof");
        let player_start = std::time::Instant::now();

        // Player shuffles with proof
        let shuffle_start = std::time::Instant::now();
        let player_shuffle = shuffle(&mut self.rng, &self.encrypted_deck, &self.aggregated_pk);
        log::info!("Player shuffle (crypto only) took {}s", shuffle_start.elapsed().as_secs());

        let proof_gen_start = std::time::Instant::now();
        let player_proof = generate_shuffle_proof_rapidsnark(
            &player_shuffle.public_inputs,
            player_shuffle.private_inputs,
        )?;
        log::info!("Player proof generation took {}s", proof_gen_start.elapsed().as_secs());

        // Verify player's shuffle proof
        let verify_start = std::time::Instant::now();
        if !verify_shuffle_proof_rapidsnark(shuffle_vkey_path, &player_proof, &player_shuffle.public_inputs)? {
            return Err("Player shuffle proof verification failed".into());
        }
        log::info!("Player proof verification took {}s", verify_start.elapsed().as_secs());
        log::info!("Player shuffle proof completed in {}s", player_start.elapsed().as_secs());

        self.encrypted_deck = player_shuffle.deck;

        log::info!("Generating dealer shuffle proof");
        let dealer_start = std::time::Instant::now();

        // Dealer shuffles with proof
        let shuffle_start = std::time::Instant::now();
        let dealer_shuffle = shuffle(&mut self.rng, &self.encrypted_deck, &self.aggregated_pk);
        log::info!("Dealer shuffle (crypto only) took {}s", shuffle_start.elapsed().as_secs());

        let proof_gen_start = std::time::Instant::now();
        let dealer_proof = generate_shuffle_proof_rapidsnark(
            &dealer_shuffle.public_inputs,
            dealer_shuffle.private_inputs,
        )?;
        log::info!("Dealer proof generation took {}s", proof_gen_start.elapsed().as_secs());

        // Verify dealer's shuffle proof
        let verify_start = std::time::Instant::now();
        if !verify_shuffle_proof_rapidsnark(shuffle_vkey_path, &dealer_proof, &dealer_shuffle.public_inputs)? {
            return Err("Dealer shuffle proof verification failed".into());
        }
        log::info!("Dealer proof verification took {}s", verify_start.elapsed().as_secs());
        log::info!("Dealer shuffle proof completed in {}s", dealer_start.elapsed().as_secs());

        self.encrypted_deck = dealer_shuffle.deck;

        Ok(())
    }

    pub fn draw_card(&mut self, for_dealer: bool, spot_index: Option<usize>) -> Result<(), Box<dyn std::error::Error>> {
        if self.deck_position >= self.encrypted_deck.len() {
            return Err("No more cards in deck".into());
        }

        let card_to_reveal = &self.encrypted_deck[self.deck_position];
        self.deck_position += 1;

        // Both parties reveal
        let player_reveal = reveal_card(&self.player_keys.sk, card_to_reveal, &self.player_keys.pk);
        let dealer_reveal = reveal_card(&self.dealer_keys.sk, card_to_reveal, &self.dealer_keys.pk);

        // Combine partial decryptions
        let combined_reveal = (player_reveal.partial_decryption.into_group() +
                             dealer_reveal.partial_decryption.into_group()).into_affine();
        let revealed_card_point = (card_to_reveal.c1.into_group() - combined_reveal.into_group()).into_affine();

        // Find which card it is
        let card_index = self.card_mapping.iter()
            .position(|&point| point == revealed_card_point)
            .ok_or("Card not found in mapping")?;

        let card = Card::from_index(card_index);

        if for_dealer {
            self.dealer_hand.push(Some(card));
        } else {
            let spot = spot_index.unwrap_or(0);
            if spot >= self.num_spots {
                return Err(format!("Invalid spot index: {}", spot).into());
            }
            // Add to the active hand within the spot
            let hand_index = self.active_hand_in_spot;
            if hand_index >= self.player_hands[spot].len() {
                return Err(format!("Invalid hand index: {}", hand_index).into());
            }
            self.player_hands[spot][hand_index].push(Some(card));
        }

        Ok(())
    }

    pub fn calculate_hand_value(hand: &[Option<Card>]) -> u8 {
        let mut total = 0;
        let mut aces = 0;

        for card_opt in hand {
            if let Some(card) = card_opt {
                let value = card.value();
                if value == 11 {
                    aces += 1;
                }
                total += value;
            }
        }

        // Adjust for aces
        while total > 21 && aces > 0 {
            total -= 10;
            aces -= 1;
        }

        total
    }

    pub fn dealer_should_hit(&self) -> bool {
        let dealer_value = Self::calculate_hand_value(&self.dealer_hand);
        dealer_value < 17
    }

    pub fn resize_for_spots(&mut self, num_spots: usize) -> Result<(), Box<dyn std::error::Error>> {
        if num_spots == 0 || num_spots > 8 {
            return Err("Number of spots must be between 1 and 8".into());
        }

        self.num_spots = num_spots;
        self.player_hands.resize(num_spots, vec![Vec::new()]);
        self.hands_doubled.resize(num_spots, vec![false]);
        self.hands_stood.resize(num_spots, vec![false]);
        self.active_spot = 0;
        self.active_hand_in_spot = 0;

        Ok(())
    }

    fn get_current_hand(&self) -> &Vec<Option<Card>> {
        &self.player_hands[self.active_spot][self.active_hand_in_spot]
    }

    pub fn can_double(&self) -> bool {
        // Can only double on first action (2 cards) for current hand
        let current_hand = self.get_current_hand();
        current_hand.len() == 2 && !self.hands_doubled[self.active_spot][self.active_hand_in_spot]
    }

    pub fn can_split(&self) -> bool {
        // Can only split if current hand has exactly 2 cards of same rank
        // and spot hasn't been split already (only allow one split per spot)
        let spot_hands = &self.player_hands[self.active_spot];
        if spot_hands.len() > 1 {
            return false; // Already split
        }

        let current_hand = self.get_current_hand();
        if current_hand.len() != 2 {
            return false;
        }

        if let (Some(card1), Some(card2)) = (&current_hand[0], &current_hand[1]) {
            card1.rank() == card2.rank()
        } else {
            false
        }
    }

    pub fn split(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.can_split() {
            return Err("Cannot split".into());
        }

        let spot = self.active_spot;
        let hand = &mut self.player_hands[spot][0];

        // Remove the second card from the first hand
        let second_card = hand.pop().ok_or("No second card to split")?;

        // Create a new hand with the second card
        self.player_hands[spot].push(vec![second_card]);

        // Initialize doubled/stood for the new hand
        self.hands_doubled[spot].push(false);
        self.hands_stood[spot].push(false);

        // Deal one card to each hand
        self.active_hand_in_spot = 0;
        self.draw_card(false, Some(spot))?;
        self.active_hand_in_spot = 1;
        self.draw_card(false, Some(spot))?;

        // Reset to first hand
        self.active_hand_in_spot = 0;

        Ok(())
    }

    pub fn is_current_hand_busted(&self) -> bool {
        Self::calculate_hand_value(self.get_current_hand()) > 21
    }

    pub fn move_to_next_hand_or_spot(&mut self) -> bool {
        // Move to next hand within spot (if split)
        let num_hands_in_spot = self.player_hands[self.active_spot].len();
        if self.active_hand_in_spot + 1 < num_hands_in_spot {
            self.active_hand_in_spot += 1;
            return true; // More hands in this spot
        }

        // Move to next spot
        self.active_spot += 1;
        self.active_hand_in_spot = 0;
        self.active_spot < self.num_spots
    }

    pub fn double_down(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.can_double() {
            return Err("Cannot double down".into());
        }

        self.hands_doubled[self.active_spot][self.active_hand_in_spot] = true;
        self.hands_stood[self.active_spot][self.active_hand_in_spot] = true; // Auto-stand after double
        self.draw_card(false, Some(self.active_spot))?; // Draw one more card

        Ok(())
    }
}
