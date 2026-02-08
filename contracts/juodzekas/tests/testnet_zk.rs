use cosmwasm_std::{Binary, Uint128};
use juodzekas::msg::{ExecuteMsg, QueryMsg, GameResponse};
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

// Environment variables will be loaded from .env file or system environment
fn get_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{key} must be set"))
}

fn query_contract<T: serde::de::DeserializeOwned>(client: &Client, address: &str, msg: &QueryMsg) -> anyhow::Result<T> {
    let query_bytes = serde_json_wasm::to_vec(msg)?;
    
    // Manual ABCI query since query_smart_contract was removed from Client
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

#[test]
fn test_testnet_zk_game_flow() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::try_init().ok();

    let rpc_url = get_env("RPC_URL");
    let _chain_id = get_env("CHAIN_ID");
    let contract_addr = get_env("CONTRACT_ADDR");
    let dealer_mnemonic = get_env("DEALER_MNEMONIC");
    let player_mnemonic = get_env("PLAYER_MNEMONIC");

    let mut rng = ChaCha8Rng::seed_from_u64(42);

    // 1. Setup Wallets
    println!("Setting up wallets...");
    let dealer_signer = Arc::new(RustSigner::from_mnemonic(dealer_mnemonic, "xion".to_string(), None)?);
    let player_signer = Arc::new(RustSigner::from_mnemonic(player_mnemonic, "xion".to_string(), None)?);

    let config = ChainConfig::new("xion-testnet-2".to_string(), rpc_url, "xion".to_string());

    let dealer_client = Client::new_with_signer(config.clone(), dealer_signer.clone())?;
    let player_client = Client::new_with_signer(config, player_signer.clone())?;

    println!("Dealer: {}", dealer_signer.address());
    println!("Player: {}", player_signer.address());

    // 2. Generate ZK Keys and Initial Deck
    let dealer_keys = KeyPair::generate(&mut rng);
    let player_keys = KeyPair::generate(&mut rng);
    let aggregated_pk = (dealer_keys.pk.into_group() + player_keys.pk.into_group()).into_affine();

    let g = Point::generator();
    let mut cards = Vec::new();
    for i in 0..52 {
        let card_point = (g * Fr::from(i as u64)).into_affine();
        cards.push(card_point);
    }

    let initial_deck: Vec<Ciphertext> = cards.iter().map(|m| {
        let r = Fr::rand(&mut rng);
        encrypt(&aggregated_pk, m, &r)
    }).collect();

    // 3. Dealer: Create Game
    println!("Dealer: Shuffling and generating proof...");
    let dealer_shuffle_result = shuffle(&mut rng, &initial_deck, &aggregated_pk);
    
    // Wrapped in a spawn_blocking or similar if necessary, but rapidsnark is synchronous.
    // However, it might be calling something that expects a runtime.
    let dealer_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            generate_shuffle_proof_rapidsnark(
                &dealer_shuffle_result.public_inputs,
                dealer_shuffle_result.private_inputs.clone(),
            )
        })
        .map_err(|e| anyhow::anyhow!("Dealer proof error: {e}"))?;

    let create_msg = ExecuteMsg::CreateGame {
        public_key: serialize_ark(&dealer_keys.pk),
        shuffled_deck: dealer_shuffle_result.deck.iter()
            .map(serialize_ciphertext)
            .collect(),
        proof: Binary::new(serde_json::to_vec(&dealer_proof)?),
        public_inputs: dealer_shuffle_result.public_inputs.to_ark_public_inputs()
            .iter().map(|f| f.into_bigint().to_string()).collect(),
    };

    println!("Dealer: Sending CreateGame...");
    let create_msg_bytes = serde_json_wasm::to_vec(&create_msg)?;
    
    let config: juodzekas::state::Config = query_contract(&dealer_client, &contract_addr, &QueryMsg::GetConfig {})?;
    let required_bankroll = config.max_bet.u128() * 10;
    
    let create_tx = dealer_client.execute_contract(
        contract_addr.clone(), 
        create_msg_bytes, 
        vec![mob::Coin::new(config.denom.clone(), required_bankroll.to_string())], 
        None
    ).map_err(|e| anyhow::anyhow!("CreateGame TX error: {e}"))?;
    println!("CreateGame TX: {}", create_tx.txhash);

    // Get TX details to see if it failed on-chain
    println!("Checking TX status...");
    for _ in 0..10 {
        match dealer_client.get_tx(create_tx.txhash.clone()) {
            Ok(tx_resp) => {
                println!("TX status found: code={}", tx_resp.code);
                if tx_resp.code != 0 {
                    println!("TX failed: {}", tx_resp.raw_log);
                }
                break;
            }
            Err(_) => {
                println!("TX not found yet...");
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }

    // Wait for the block to be committed
    println!("Waiting for game to be indexed...");
    std::thread::sleep(std::time::Duration::from_secs(6));

    let games: Vec<juodzekas::msg::GameListItem> = query_contract(&dealer_client, &contract_addr, &QueryMsg::ListGames { status_filter: None })?;
    println!("Games found total: {}", games.len());
    for g in &games {
        println!("  Game ID: {}, Dealer: {}, Status: {}", g.game_id, g.dealer, g.status);
    }
    
    // Extract game_id from events (simplified for this test)
    let mut game_id = None;
    for i in 0..15 {
        let games: Vec<juodzekas::msg::GameListItem> = query_contract(&dealer_client, &contract_addr, &QueryMsg::ListGames { status_filter: None })?;
        if let Some(g) = games.iter().find(|g| g.dealer == dealer_signer.address()) {
            game_id = Some(g.game_id);
            break;
        }
        println!("Game not found yet (attempt {}), retrying...", i + 1);
        std::thread::sleep(std::time::Duration::from_secs(4));
    }

    let game_id = game_id.ok_or_else(|| anyhow::anyhow!("Game not found after retries"))?;
    println!("Game ID: {game_id}");

    // 4. Player: Join Game
    println!("Player: Shuffling and generating proof...");
    let player_shuffle_result = shuffle(&mut rng, &dealer_shuffle_result.deck, &aggregated_pk);
    
    let player_proof = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            generate_shuffle_proof_rapidsnark(
                &player_shuffle_result.public_inputs,
                player_shuffle_result.private_inputs.clone(),
            )
        })
        .map_err(|e| anyhow::anyhow!("Player proof error: {e}"))?;

    let join_msg = ExecuteMsg::JoinGame {
        game_id,
        bet: Uint128::new(1000),
        public_key: serialize_ark(&player_keys.pk),
        shuffled_deck: player_shuffle_result.deck.iter()
            .map(serialize_ciphertext)
            .collect(),
        proof: Binary::new(serde_json::to_vec(&player_proof)?),
        public_inputs: player_shuffle_result.public_inputs.to_ark_public_inputs()
            .iter().map(|f| f.into_bigint().to_string()).collect(),
    };

    println!("Player: Sending JoinGame...");
    let join_msg_bytes = serde_json_wasm::to_vec(&join_msg)?;
    let join_tx = player_client.execute_contract(
        contract_addr.clone(), 
        join_msg_bytes, 
        vec![mob::Coin::new("uxion".to_string(), 1000u128.to_string())], 
        None
    ).map_err(|e| anyhow::anyhow!("JoinGame TX error: {e}"))?;
    println!("JoinGame TX: {}", join_tx.txhash);

    // 5. Query Game State
    println!("Waiting for join to be processed...");
    std::thread::sleep(std::time::Duration::from_secs(6));

    let mut game: GameResponse = query_contract(&player_client, &contract_addr, &QueryMsg::GetGame { game_id })?;
    println!("Game Status: {}", game.status);

    // 6. Reveal Cards (Player & Dealer)
    // The contract expects reveals for card 0, 1 (player) and 2 (dealer upcard)
    // Initially card 3 (dealer hole card) is NOT requested until dealer turn.

    if game.status.contains("WaitingForReveal") {
        // Parse requested cards from game.status if we had a better parser, 
        // but we know it's [0, 1, 2] from JoinGame implementation
        let card_indices = vec![0u32, 1u32, 2u32];

        for card_idx in card_indices {
            println!("Revealing card {card_idx}...");
            let ciphertext = deserialize_ciphertext(&game.deck[card_idx as usize]);

            // Both must reveal
            // Dealer reveals
            let dealer_reveal = reveal_card(&dealer_keys.sk, &ciphertext, &dealer_keys.pk);
            let dealer_reveal_proof = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap()
                .block_on(async {
                    generate_reveal_proof_rapidsnark(&dealer_reveal.public_inputs, dealer_reveal.sk_p)
                }).map_err(|e| anyhow::anyhow!("Dealer reveal proof error: {e}"))?;

            let reveal_msg_dealer = ExecuteMsg::SubmitReveal {
                game_id,
                card_index: card_idx,
                partial_decryption: serialize_ark(&dealer_reveal.partial_decryption),
                proof: Binary::new(serde_json::to_vec(&dealer_reveal_proof)?),
                public_inputs: dealer_reveal.public_inputs.to_ark_public_inputs()
                    .iter().map(|f| f.into_bigint().to_string()).collect(),
            };
            dealer_client.execute_contract(contract_addr.clone(), serde_json_wasm::to_vec(&reveal_msg_dealer)?, vec![], None)?;

            // Player reveals
            let player_reveal = reveal_card(&player_keys.sk, &ciphertext, &player_keys.pk);
            let player_reveal_proof = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap()
                .block_on(async {
                    generate_reveal_proof_rapidsnark(&player_reveal.public_inputs, player_reveal.sk_p)
                }).map_err(|e| anyhow::anyhow!("Player reveal proof error: {e}"))?;

            let reveal_msg_player = ExecuteMsg::SubmitReveal {
                game_id,
                card_index: card_idx,
                partial_decryption: serialize_ark(&player_reveal.partial_decryption),
                proof: Binary::new(serde_json::to_vec(&player_reveal_proof)?),
                public_inputs: player_reveal.public_inputs.to_ark_public_inputs()
                    .iter().map(|f| f.into_bigint().to_string()).collect(),
            };
            player_client.execute_contract(contract_addr.clone(), serde_json_wasm::to_vec(&reveal_msg_player)?, vec![], None)?;

            println!("Card {card_idx} revealed");
            std::thread::sleep(std::time::Duration::from_secs(4));
        }
    }

    // Final state
    std::thread::sleep(std::time::Duration::from_secs(6));
    game = query_contract(&player_client, &contract_addr, &QueryMsg::GetGame { game_id })?;
    println!("Final Game Status: {}", game.status);
    println!("Player Hands: {:?}", game.hands);
    println!("Dealer Hand: {:?}", game.dealer_hand);

    Ok(())
}
