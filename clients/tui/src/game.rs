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

// Re-export from blackjack package
pub use blackjack::{Card, GameRules};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GameMode {
    Fast,      // No ZK proofs, instant gameplay
    Trustless, // Full ZK proofs, takes ~3-4 minutes to start
    Contract,  // Full ZK proofs + on-chain smart contract
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
    pub rules: GameRules, // Game rules configuration
    pub active_spot: usize, // Current spot being played (0-indexed)
    pub active_hand_in_spot: usize, // When spot is split, which hand within spot (0-indexed)
    pub hands_doubled: Vec<Vec<bool>>, // Track which hands have doubled [spot][hand_in_spot]
    pub hands_stood: Vec<Vec<bool>>, // Track which hands have stood [spot][hand_in_spot]
    pub hands_surrendered: Vec<Vec<bool>>, // Track which hands have surrendered [spot][hand_in_spot]
    pub dealer_peeked: bool, // Whether dealer has peeked for blackjack
}

impl GameState {
    pub fn new(mode: GameMode, num_spots: usize) -> Result<Self, Box<dyn std::error::Error>> {
        if num_spots == 0 || num_spots > 8 {
            return Err("Number of spots must be between 1 and 8".into());
        }

        Self::new_with_spots(mode, num_spots)
    }

    pub fn new_uninitialized(mode: GameMode) -> Result<Self, Box<dyn std::error::Error>> {
        // Create with 1 spot as placeholder - will be resized before dealing
        Self::new_with_spots(mode, 1)
    }

    fn new_with_spots(mode: GameMode, num_spots: usize) -> Result<Self, Box<dyn std::error::Error>> {

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
        let hands_surrendered = vec![vec![false]; num_spots];

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
            rules: GameRules::default(), // Use default (Las Vegas) rules
            active_spot: 0,
            active_hand_in_spot: 0,
            hands_doubled,
            hands_stood,
            hands_surrendered,
            dealer_peeked: false,
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
                return Err(format!("Invalid spot index: {spot}").into());
            }
            // Add to the active hand within the spot
            let hand_index = self.active_hand_in_spot;
            if hand_index >= self.player_hands[spot].len() {
                return Err(format!("Invalid hand index: {hand_index}").into());
            }
            self.player_hands[spot][hand_index].push(Some(card));
        }

        Ok(())
    }

    pub fn calculate_hand_value(hand: &[Option<Card>]) -> u8 {
        // Filter out None values and call blackjack package's calculate_hand_value
        let cards: Vec<Card> = hand.iter().filter_map(|&c| c).collect();
        blackjack::calculate_hand_value(&cards)
    }

    pub fn dealer_should_hit(&self) -> bool {
        // Use blackjack package logic which respects soft 17 rules
        self.should_dealer_hit()
    }

    pub fn resize_for_spots(&mut self, num_spots: usize) -> Result<(), Box<dyn std::error::Error>> {
        if num_spots == 0 || num_spots > 8 {
            return Err("Number of spots must be between 1 and 8".into());
        }

        self.num_spots = num_spots;
        self.player_hands.resize(num_spots, vec![Vec::new()]);
        self.hands_doubled.resize(num_spots, vec![false]);
        self.hands_stood.resize(num_spots, vec![false]);
        self.hands_surrendered.resize(num_spots, vec![false]);
        self.active_spot = 0;
        self.active_hand_in_spot = 0;
        self.dealer_peeked = false;

        Ok(())
    }

    fn get_current_hand(&self) -> &Vec<Option<Card>> {
        &self.player_hands[self.active_spot][self.active_hand_in_spot]
    }

    pub fn can_double(&self) -> bool {
        // Use blackjack package logic which respects double after split rules
        self.can_double_current_hand()
    }

    pub fn can_split(&self) -> bool {
        // Use blackjack package logic which respects max splits and resplit aces rules
        self.can_split_current_hand()
    }

    pub fn split(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.can_split() {
            return Err("Cannot split".into());
        }

        let spot = self.active_spot;
        let hand_idx = self.active_hand_in_spot;
        let hand = &mut self.player_hands[spot][hand_idx];

        // Remove the second card from the active hand
        let second_card = hand.pop().ok_or("No second card to split")?;

        // Create a new hand with the second card (inserted right after the active hand)
        let new_hand_idx = hand_idx + 1;
        self.player_hands[spot].insert(new_hand_idx, vec![second_card]);

        // Initialize doubled/stood/surrendered for the new hand
        self.hands_doubled[spot].insert(new_hand_idx, false);
        self.hands_stood[spot].insert(new_hand_idx, false);
        self.hands_surrendered[spot].insert(new_hand_idx, false);

        // Deal one card to the original hand, one to the new hand
        self.active_hand_in_spot = hand_idx;
        self.draw_card(false, Some(spot))?;
        self.active_hand_in_spot = new_hand_idx;
        self.draw_card(false, Some(spot))?;

        // Reset to the original hand
        self.active_hand_in_spot = hand_idx;

        Ok(())
    }


    // These methods now delegate to game_logic.rs which uses blackjack package
    // Kept here for backwards compatibility with existing TUI code

    pub fn can_surrender(&self) -> bool {
        // Use blackjack package logic which respects surrender rules and late surrender
        self.can_surrender_current_hand()
    }

    pub fn surrender(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.can_surrender() {
            return Err("Cannot surrender".into());
        }

        self.hands_surrendered[self.active_spot][self.active_hand_in_spot] = true;
        self.hands_stood[self.active_spot][self.active_hand_in_spot] = true; // Also mark as stood to skip

        Ok(())
    }

    pub fn get_optimal_move(&self) -> &'static str {
        let hand = self.get_current_hand();
        let player_value = Self::calculate_hand_value(hand);

        // Get dealer's up card value
        let dealer_up_card = match self.dealer_hand.first() {
            Some(Some(card)) => card.value(),
            _ => return "Stand", // No dealer card visible
        };

        // Check for soft hand (Ace counted as 11)
        let cards: Vec<Card> = hand.iter().filter_map(|&c| c).collect();
        let is_soft = blackjack::is_soft_hand(&cards);

        // Check for surrender (before split/double)
        if self.can_surrender() {
            // Surrender on hard 16 vs dealer 9, 10, A
            // Surrender on hard 15 vs dealer 10
            if !is_soft {
                if player_value == 16 && (dealer_up_card == 9 || dealer_up_card == 10 || dealer_up_card == 11) {
                    return "Surrender";
                }
                if player_value == 15 && dealer_up_card == 10 {
                    return "Surrender";
                }
            }
        }

        // Check if can split
        if self.can_split() {
            let card_rank = if let Some(card) = &hand[0] {
                card.rank()
            } else {
                return "Stand";
            };

            // Always split Aces and 8s
            if card_rank == 1 || card_rank == 8 {
                return "Split";
            }
            // Never split 10s, 5s, 4s
            if card_rank == 10 || card_rank == 11 || card_rank == 12 || card_rank == 13 || card_rank == 5 || card_rank == 4 {
                // Fall through to regular strategy
            } else if card_rank == 9 {
                // Split 9s except against 7, 10, or Ace
                if dealer_up_card != 7 && dealer_up_card != 10 && dealer_up_card != 11 {
                    return "Split";
                }
            } else if card_rank == 7 || card_rank == 6 {
                // Split 7s and 6s against 2-7
                if (2..=7).contains(&dealer_up_card) {
                    return "Split";
                }
            } else if card_rank == 3 || card_rank == 2 {
                // Split 2s and 3s against 2-7
                if (2..=7).contains(&dealer_up_card) {
                    return "Split";
                }
            }
        }

        // Check if can double
        if self.can_double() {
            if is_soft {
                // Soft doubling
                if (player_value == 19 && dealer_up_card == 6)
                    || (player_value == 18 && (2..=6).contains(&dealer_up_card))
                    || (player_value == 17 && (3..=6).contains(&dealer_up_card))
                    || ((15..=16).contains(&player_value) && (4..=6).contains(&dealer_up_card))
                    || ((13..=14).contains(&player_value) && (5..=6).contains(&dealer_up_card))
                {
                    return "Double";
                }
            } else {
                // Hard doubling
                if player_value == 11
                    || (player_value == 10 && dealer_up_card <= 9)
                    || (player_value == 9 && (3..=6).contains(&dealer_up_card))
                {
                    return "Double";
                }
            }
        }

        // Basic strategy for hitting/standing
        if is_soft {
            // Soft hands
            if player_value >= 19 {
                "Stand"
            } else if player_value == 18 {
                if dealer_up_card >= 9 {
                    "Hit"
                } else {
                    "Stand"
                }
            } else {
                "Hit"
            }
        } else {
            // Hard hands
            if player_value >= 17 {
                "Stand"
            } else if (13..=16).contains(&player_value) {
                if (2..=6).contains(&dealer_up_card) {
                    "Stand"
                } else {
                    "Hit"
                }
            } else if player_value == 12 {
                if (4..=6).contains(&dealer_up_card) {
                    "Stand"
                } else {
                    "Hit"
                }
            } else {
                "Hit"
            }
        }
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
