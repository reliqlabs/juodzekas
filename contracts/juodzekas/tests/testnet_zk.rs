use cosmwasm_std::{Binary, Uint128};
use juodzekas::msg::{DealerBalanceResponse, ExecuteMsg, QueryMsg, GameResponse};
use mob::{ChainConfig, Client, RustSigner};
use std::sync::Arc;
use std::env;
use dotenvy::dotenv;
use zk_shuffle::elgamal::{KeyPair, encrypt, Ciphertext};
use zk_shuffle::shuffle::shuffle;
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::babyjubjub::{Point, Fr};
use zk_shuffle::proof::{
    generate_shuffle_proof_rapidsnark, generate_reveal_proof_rapidsnark,
    CanonicalSerialize, CanonicalDeserialize,
};
use ark_ec::{CurveGroup, AffineRepr};
use ark_ff::{PrimeField, UniformRand};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

fn serialize_ark<T: CanonicalSerialize>(obj: &T) -> Binary {
    let mut buf = Vec::new();
    obj.serialize_compressed(&mut buf).unwrap();
    Binary::new(buf)
}

fn serialize_ciphertext(c: &Ciphertext) -> Binary {
    let mut buf = Vec::new();
    c.c0.serialize_compressed(&mut buf).unwrap();
    c.c1.serialize_compressed(&mut buf).unwrap();
    Binary::new(buf)
}

fn deserialize_ciphertext(b: &Binary) -> Ciphertext {
    let mut buf = b.as_slice();
    let c0 = Point::deserialize_compressed(&mut buf).unwrap();
    let c1 = Point::deserialize_compressed(&mut buf).unwrap();
    Ciphertext { c0, c1 }
}

fn get_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} must be set"))
}

fn query_contract<T: serde::de::DeserializeOwned>(client: &Client, address: &str, msg: &QueryMsg) -> anyhow::Result<T> {
    let query_bytes = serde_json_wasm::to_vec(msg)?;

    let path = "/cosmwasm.wasm.v1.Query/SmartContractState";
    let data = {
        use prost::Message;
        let req = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateRequest {
            address: address.to_string(),
            query_data: query_bytes,
        };
        req.encode_to_vec()
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let response = runtime.block_on(async {
        use tendermint_rpc::{Client as TmClient, HttpClient};
        let rpc_url = client.config().rpc_endpoint;
        let tm_client = HttpClient::new(rpc_url.as_str())?;
        tm_client.abci_query(Some(path.to_string()), data, None, false).await
    })?;

    if response.code.is_err() {
        return Err(anyhow::anyhow!("ABCI query failed: {}", response.log));
    }

    use prost::Message;
    let res_wrapper = xion_types::cosmwasm::wasm::v1::QuerySmartContractStateResponse::decode(response.value.as_slice())?;
    let res = serde_json_wasm::from_slice(&res_wrapper.data)?;
    Ok(res)
}

/// Poll until TX is confirmed on-chain. Fails if TX executes with non-zero code.
fn confirm_tx(client: &Client, txhash: &str) -> anyhow::Result<()> {
    for attempt in 0..15 {
        match client.get_tx(txhash.to_string()) {
            Ok(tx_resp) => {
                if tx_resp.code != 0 {
                    return Err(anyhow::anyhow!(
                        "TX failed: code={}, log={}", tx_resp.code, tx_resp.raw_log
                    ));
                }
                return Ok(());
            }
            Err(_) => {
                if attempt < 14 {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }
    Err(anyhow::anyhow!("TX not confirmed after 30s: {txhash}"))
}

/// Extract card indices from debug-formatted WaitingForReveal status string.
fn parse_reveal_requests(status: &str) -> Vec<u32> {
    if let Some(start) = status.find("reveal_requests: [") {
        let after = &status[start + "reveal_requests: [".len()..];
        if let Some(end) = after.find(']') {
            return after[..end]
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
        }
    }
    vec![]
}

/// Submit reveal for a single card from both dealer and player, confirming each TX.
fn reveal_card_on_chain(
    dealer_client: &Client,
    player_client: &Client,
    contract_addr: &str,
    game_id: u64,
    card_idx: u32,
    dealer_keys: &KeyPair,
    player_keys: &KeyPair,
    deck: &[Binary],
) -> anyhow::Result<()> {
    let ciphertext = deserialize_ciphertext(&deck[card_idx as usize]);

    // Dealer reveal
    let d_reveal = reveal_card(&dealer_keys.sk, &ciphertext, &dealer_keys.pk);
    let d_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all().build()?
        .block_on(async {
            generate_reveal_proof_rapidsnark(&d_reveal.public_inputs, d_reveal.sk_p)
        }).map_err(|e| anyhow::anyhow!("Dealer reveal proof: {e}"))?;

    let tx = dealer_client.execute_contract(
        contract_addr.to_string(),
        serde_json_wasm::to_vec(&ExecuteMsg::SubmitReveal {
            game_id,
            card_index: card_idx,
            partial_decryption: serialize_ark(&d_reveal.partial_decryption),
            proof: Binary::new(serde_json::to_vec(&d_proof)?),
            public_inputs: d_reveal.public_inputs.to_ark_public_inputs()
                .iter().map(|f| f.into_bigint().to_string()).collect(),
        })?,
        vec![], None, None,
    ).map_err(|e| anyhow::anyhow!("Dealer reveal TX: {e}"))?;
    confirm_tx(dealer_client, &tx.txhash)?;

    // Player reveal
    let p_reveal = reveal_card(&player_keys.sk, &ciphertext, &player_keys.pk);
    let p_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all().build()?
        .block_on(async {
            generate_reveal_proof_rapidsnark(&p_reveal.public_inputs, p_reveal.sk_p)
        }).map_err(|e| anyhow::anyhow!("Player reveal proof: {e}"))?;

    let tx = player_client.execute_contract(
        contract_addr.to_string(),
        serde_json_wasm::to_vec(&ExecuteMsg::SubmitReveal {
            game_id,
            card_index: card_idx,
            partial_decryption: serialize_ark(&p_reveal.partial_decryption),
            proof: Binary::new(serde_json::to_vec(&p_proof)?),
            public_inputs: p_reveal.public_inputs.to_ark_public_inputs()
                .iter().map(|f| f.into_bigint().to_string()).collect(),
        })?,
        vec![], None, None,
    ).map_err(|e| anyhow::anyhow!("Player reveal TX: {e}"))?;
    confirm_tx(player_client, &tx.txhash)?;

    Ok(())
}

/// Card value (0-51) to blackjack score for display purposes.
fn card_score(val: u8) -> u8 {
    let rank = (val % 13) + 1;
    if rank == 1 { 11 } else if rank > 10 { 10 } else { rank }
}

fn hand_score(cards: &[u8]) -> u8 {
    let mut score: u8 = 0;
    let mut aces: u8 = 0;
    for &c in cards {
        let s = card_score(c);
        if s == 11 { aces += 1; }
        score = score.saturating_add(s);
    }
    while score > 21 && aces > 0 {
        score -= 10;
        aces -= 1;
    }
    score
}

/// Run a full game on testnet with the given seed. Player always stands immediately.
fn run_testnet_game(seed: u64) -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::try_init().ok();

    let rpc_url = get_env("RPC_URL");
    let contract_addr = get_env("CONTRACT_ADDR");
    let dealer_mnemonic = get_env("DEALER_MNEMONIC");
    let player_mnemonic = get_env("PLAYER_MNEMONIC");

    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // 1. Setup wallets
    println!("=== Seed {seed} ===");
    println!("Setting up wallets...");
    let dealer_signer = Arc::new(RustSigner::from_mnemonic(dealer_mnemonic, "xion".to_string(), None)?);
    let player_signer = Arc::new(RustSigner::from_mnemonic(player_mnemonic, "xion".to_string(), None)?);
    let config = ChainConfig::new("xion-testnet-2".to_string(), rpc_url, "xion".to_string());
    let dealer_client = Client::new_with_signer(config.clone(), dealer_signer.clone())?;
    let player_client = Client::new_with_signer(config, player_signer.clone())?;
    println!("Dealer: {}", dealer_signer.address());
    println!("Player: {}", player_signer.address());

    // 2. Generate keys (deterministic from seed)
    let dealer_keys = KeyPair::generate(&mut rng);
    let player_keys = KeyPair::generate(&mut rng);
    let aggregated_pk = (dealer_keys.pk.into_group() + player_keys.pk.into_group()).into_affine();

    // Initial plaintext deck
    let g = Point::generator();
    let cards: Vec<_> = (0..52).map(|i| (g * Fr::from(i as u64)).into_affine()).collect();
    let initial_deck: Vec<Ciphertext> = cards.iter().map(|m| {
        let r = Fr::rand(&mut rng);
        encrypt(&aggregated_pk, m, &r)
    }).collect();

    // 3. Dealer shuffle + proof
    println!("Dealer: Shuffling...");
    let dealer_shuffle = shuffle(&mut rng, &initial_deck, &aggregated_pk);
    let dealer_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all().build()?
        .block_on(async {
            generate_shuffle_proof_rapidsnark(
                &dealer_shuffle.public_inputs,
                dealer_shuffle.private_inputs.clone(),
            )
        }).map_err(|e| anyhow::anyhow!("Dealer shuffle proof: {e}"))?;

    // 4. CreateGame
    println!("Dealer: CreateGame...");
    let contract_config: juodzekas::state::Config =
        query_contract(&dealer_client, &contract_addr, &QueryMsg::GetConfig {})?;
    let bankroll = contract_config.max_bet.u128() * 10;

    let tx = dealer_client.execute_contract(
        contract_addr.clone(),
        serde_json_wasm::to_vec(&ExecuteMsg::CreateGame {
            public_key: serialize_ark(&dealer_keys.pk),
            shuffled_deck: dealer_shuffle.deck.iter().map(serialize_ciphertext).collect(),
            proof: Binary::new(serde_json::to_vec(&dealer_proof)?),
            public_inputs: dealer_shuffle.public_inputs.to_ark_public_inputs()
                .iter().map(|f| f.into_bigint().to_string()).collect(),
        })?,
        vec![mob::Coin::new(contract_config.denom.clone(), bankroll.to_string())],
        None,
        Some(800_000),
    ).map_err(|e| anyhow::anyhow!("CreateGame TX: {e}"))?;
    println!("CreateGame TX: {}", tx.txhash);
    confirm_tx(&dealer_client, &tx.txhash)?;
    println!("CreateGame confirmed");

    // Find game ID
    std::thread::sleep(std::time::Duration::from_secs(4));
    let games: Vec<juodzekas::msg::GameListItem> = query_contract(
        &dealer_client, &contract_addr,
        &QueryMsg::ListGames { status_filter: Some("WaitingForPlayerJoin".to_string()) },
    )?;
    let game_id = games.iter()
        .filter(|g| g.dealer == dealer_signer.address())
        .map(|g| g.game_id)
        .max()
        .ok_or_else(|| anyhow::anyhow!("No WaitingForPlayerJoin game found for dealer"))?;
    println!("Game ID: {game_id}");

    // 5. Player shuffle + proof
    println!("Player: Shuffling...");
    let player_shuffle = shuffle(&mut rng, &dealer_shuffle.deck, &aggregated_pk);
    let player_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all().build()?
        .block_on(async {
            generate_shuffle_proof_rapidsnark(
                &player_shuffle.public_inputs,
                player_shuffle.private_inputs.clone(),
            )
        }).map_err(|e| anyhow::anyhow!("Player shuffle proof: {e}"))?;

    // 6. JoinGame
    println!("Player: JoinGame...");
    let tx = player_client.execute_contract(
        contract_addr.clone(),
        serde_json_wasm::to_vec(&ExecuteMsg::JoinGame {
            game_id,
            bet: Uint128::new(1000),
            public_key: serialize_ark(&player_keys.pk),
            shuffled_deck: player_shuffle.deck.iter().map(serialize_ciphertext).collect(),
            proof: Binary::new(serde_json::to_vec(&player_proof)?),
            public_inputs: player_shuffle.public_inputs.to_ark_public_inputs()
                .iter().map(|f| f.into_bigint().to_string()).collect(),
        })?,
        vec![mob::Coin::new("uxion".to_string(), "1000".to_string())],
        None,
        Some(1_000_000),
    ).map_err(|e| anyhow::anyhow!("JoinGame TX: {e}"))?;
    println!("JoinGame TX: {}", tx.txhash);
    confirm_tx(&player_client, &tx.txhash)?;
    println!("JoinGame confirmed");

    // 7. Game loop: reveal cards, take actions, until settled
    let mut dealer_hit_count = 0u32;
    let max_rounds = 20;
    for round in 0..max_rounds {
        std::thread::sleep(std::time::Duration::from_secs(4));
        let game: GameResponse = query_contract(
            &player_client, &contract_addr,
            &QueryMsg::GetGame { game_id },
        )?;
        println!("\n--- Round {round} ---");
        println!("Status: {}", game.status);
        println!("Player hands: {:?}", game.hands);
        println!("Dealer hand: {:?} (score: {})", game.dealer_hand, hand_score(&game.dealer_hand));

        if game.status.contains("Settled") {
            println!("\n=== GAME SETTLED (seed {seed}) ===");
            println!("Result: {}", game.status);
            for (i, hand) in game.hands.iter().enumerate() {
                println!("  Hand {i}: cards={:?} score={} status={}", hand.cards, hand_score(&hand.cards.iter().map(|c| *c as u8).collect::<Vec<_>>()), hand.status);
            }
            println!("  Dealer: {:?} score={}", game.dealer_hand, hand_score(&game.dealer_hand));
            println!("  Dealer hit {dealer_hit_count} time(s) after hole card");

            // Assertions
            assert!(!game.hands.is_empty(), "Player should have at least one hand");
            assert!(game.hands[0].cards.len() >= 2, "Player hand needs >= 2 cards");
            assert!(game.dealer_hand.len() >= 2, "Dealer hand needs >= 2 cards");

            // Verify dealer balance was credited
            std::thread::sleep(std::time::Duration::from_secs(4));
            let dealer_bal: DealerBalanceResponse = query_contract(
                &dealer_client, &contract_addr,
                &QueryMsg::GetDealerBalance { address: dealer_signer.address() },
            )?;
            println!("Dealer balance after settlement: {}", dealer_bal.balance);
            assert!(!dealer_bal.balance.is_zero(), "Dealer should have balance after settlement");

            // Withdraw dealer balance
            let tx = dealer_client.execute_contract(
                contract_addr.clone(),
                serde_json_wasm::to_vec(&ExecuteMsg::WithdrawBankroll { amount: None })?,
                vec![], None, None,
            ).map_err(|e| anyhow::anyhow!("WithdrawBankroll TX: {e}"))?;
            println!("WithdrawBankroll TX: {}", tx.txhash);
            confirm_tx(&dealer_client, &tx.txhash)?;
            println!("WithdrawBankroll confirmed");

            // Verify balance is now zero
            std::thread::sleep(std::time::Duration::from_secs(4));
            let dealer_bal: DealerBalanceResponse = query_contract(
                &dealer_client, &contract_addr,
                &QueryMsg::GetDealerBalance { address: dealer_signer.address() },
            )?;
            assert!(dealer_bal.balance.is_zero(), "Dealer balance should be zero after withdrawal");

            return Ok(());
        } else if game.status.contains("WaitingForReveal") {
            let card_indices = parse_reveal_requests(&game.status);
            assert!(!card_indices.is_empty(), "WaitingForReveal with no cards?");

            // Track dealer hits (cards beyond index 3 when next_status is DealerTurn)
            if game.status.contains("DealerTurn") {
                for &idx in &card_indices {
                    if idx > 3 {
                        dealer_hit_count += 1;
                    }
                }
            }

            for &card_idx in &card_indices {
                println!("Revealing card {card_idx}...");
                reveal_card_on_chain(
                    &dealer_client, &player_client,
                    &contract_addr, game_id, card_idx,
                    &dealer_keys, &player_keys,
                    &game.deck,
                )?;
                println!("Card {card_idx} revealed");
            }
        } else if game.status.contains("PlayerTurn") {
            println!("Player: Stand");
            let tx = player_client.execute_contract(
                contract_addr.clone(),
                serde_json_wasm::to_vec(&ExecuteMsg::Stand { game_id })?,
                vec![], None, None,
            ).map_err(|e| anyhow::anyhow!("Stand TX: {e}"))?;
            confirm_tx(&player_client, &tx.txhash)?;
            println!("Stand confirmed");
        } else {
            panic!("Unexpected game status: {}", game.status);
        }
    }

    panic!("Game did not settle within {max_rounds} rounds");
}

/// Simulate card values locally for a given seed without touching testnet.
/// Returns (player_cards, dealer_cards) as vectors of card_value (0-51).
fn simulate_initial_deal(seed: u64) -> (Vec<u8>, Vec<u8>) {
    use ark_ff::UniformRand;

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let dealer_keys = KeyPair::generate(&mut rng);
    let player_keys = KeyPair::generate(&mut rng);
    let aggregated_pk = (dealer_keys.pk.into_group() + player_keys.pk.into_group()).into_affine();

    let g = Point::generator();
    let cards: Vec<_> = (0..52).map(|i| (g * Fr::from(i as u64)).into_affine()).collect();
    let initial_deck: Vec<Ciphertext> = cards.iter().map(|m| {
        let r = Fr::rand(&mut rng);
        encrypt(&aggregated_pk, m, &r)
    }).collect();

    let dealer_shuffle = shuffle(&mut rng, &initial_deck, &aggregated_pk);
    let player_shuffle = shuffle(&mut rng, &dealer_shuffle.deck, &aggregated_pk);

    let mut player_cards = vec![];
    let mut dealer_cards = vec![];

    // Cards 0,1 → player; cards 2,3 → dealer
    for card_idx in 0..4u32 {
        let ct = &player_shuffle.deck[card_idx as usize];
        let d_reveal = reveal_card(&dealer_keys.sk, ct, &dealer_keys.pk);
        let p_reveal = reveal_card(&player_keys.sk, ct, &player_keys.pk);

        let mut d_bytes = Vec::new();
        d_reveal.partial_decryption.serialize_compressed(&mut d_bytes).unwrap();
        let mut p_bytes = Vec::new();
        p_reveal.partial_decryption.serialize_compressed(&mut p_bytes).unwrap();

        let card_value = (p_bytes[0] ^ d_bytes[0]) % 52;
        if card_idx < 2 {
            player_cards.push(card_value);
        } else {
            dealer_cards.push(card_value);
        }
    }

    (player_cards, dealer_cards)
}

/// Contract's scoring logic, duplicated for local use.
fn contract_score(hand: &[u8]) -> u8 {
    let mut score: u8 = 0;
    let mut aces: u8 = 0;
    for &card in hand {
        let val = (card % 13) + 1;
        if val == 1 { aces += 1; score += 11; }
        else if val > 10 { score += 10; }
        else { score += val; }
    }
    while score > 21 && aces > 0 { score -= 10; aces -= 1; }
    score
}

fn is_blackjack(hand: &[u8]) -> bool {
    hand.len() == 2 && contract_score(hand) == 21
}

/// Find seeds that produce specific dealer scenarios. Run with:
///   cargo test -p juodzekas --test testnet_zk find_seeds -- --nocapture --ignored
#[test]
#[ignore]
fn find_seeds() {
    let mut dealer_hit_seed = None;
    let mut dealer_bj_seed = None;

    for seed in 0..10_000u64 {
        let (_, dealer) = simulate_initial_deal(seed);
        let d_score = contract_score(&dealer);

        if dealer_hit_seed.is_none() && d_score < 17 {
            println!("Seed {seed}: dealer={dealer:?} score={d_score} → DEALER MUST HIT");
            dealer_hit_seed = Some(seed);
        }
        if dealer_bj_seed.is_none() && is_blackjack(&dealer) {
            println!("Seed {seed}: dealer={dealer:?} score={d_score} → DEALER BLACKJACK");
            dealer_bj_seed = Some(seed);
        }
        if dealer_hit_seed.is_some() && dealer_bj_seed.is_some() {
            break;
        }
    }

    println!("\n=== Results ===");
    println!("Dealer must hit seed: {:?}", dealer_hit_seed);
    println!("Dealer blackjack seed: {:?}", dealer_bj_seed);
    assert!(dealer_hit_seed.is_some(), "No dealer-hit seed found in 0..10000");
    assert!(dealer_bj_seed.is_some(), "No dealer-blackjack seed found in 0..10000");
}

/// Seed 42: dealer score >= 17 immediately, no dealer hits. Basic flow test.
#[test]
fn test_testnet_zk_seed_42() -> anyhow::Result<()> {
    run_testnet_game(42)
}

/// Seed 7: dealer score >= 17 immediately. Different card distribution.
#[test]
fn test_testnet_zk_seed_7() -> anyhow::Result<()> {
    run_testnet_game(7)
}

/// Seed 0: dealer gets score 14 → must hit. Exercises dealer-hit code path.
#[test]
fn test_testnet_zk_dealer_hit() -> anyhow::Result<()> {
    run_testnet_game(0)
}

/// Seed 77: dealer gets blackjack (Queen + Ace = 21). Tests immediate settlement.
#[test]
fn test_testnet_zk_dealer_blackjack() -> anyhow::Result<()> {
    run_testnet_game(77)
}
