use zk_shuffle::elgamal::{KeyPair, encrypt, decrypt, Ciphertext};
use zk_shuffle::shuffle::shuffle;
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::babyjubjub::{Point, Fr};
use ark_std::UniformRand;
use ark_ec::{CurveGroup, AffineRepr};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::time::Instant;

use zk_shuffle::proof::{generate_shuffle_proof, generate_reveal_proof, verify_shuffle_proof, verify_reveal_proof, load_or_generate_keys};

#[tokio::test]
async fn test_generic_two_user_reveal_with_proofs() {
    let test_start = Instant::now();

    // 1. Setup with a fixed seed for reproducibility
    let mut rng = ChaCha8Rng::seed_from_u64(1337);

    // Load or generate proving keys
    let shuffle_r1cs = "../../circuits/artifacts/shuffle_encrypt.r1cs";
    let shuffle_wasm = "../../circuits/artifacts/shuffle_encrypt.wasm";
    let reveal_r1cs = "../../circuits/artifacts/decrypt.r1cs";
    let reveal_wasm = "../../circuits/artifacts/decrypt.wasm";

    let shuffle_pk_cache = "tests/cache/shuffle.pk";
    let shuffle_vk_cache = "tests/cache/shuffle.vk";
    let reveal_pk_cache = "tests/cache/reveal.pk";
    let reveal_vk_cache = "tests/cache/reveal.vk";

    println!("[DEBUG_LOG] Loading or Generating Proving Keys...");
    let key_load_start = Instant::now();

    let mut shuffle_placeholders = Vec::new();
    for _ in 0..2 { shuffle_placeholders.push(("pk".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("UX0".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("UX1".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("VX0".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("VX1".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("UDelta0".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("UDelta1".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("VDelta0".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("VDelta1".to_string(), 0.into())); }
    for _ in 0..2 { shuffle_placeholders.push(("s_u".to_string(), 0.into())); }
    for _ in 0..2 { shuffle_placeholders.push(("s_v".to_string(), 0.into())); }
    for _ in 0..52*52 { shuffle_placeholders.push(("A".to_string(), 0.into())); }
    for _ in 0..52 { shuffle_placeholders.push(("R".to_string(), 0.into())); }

    let (shuffle_pk, shuffle_vk) = load_or_generate_keys(
        shuffle_r1cs, shuffle_wasm, shuffle_pk_cache, shuffle_vk_cache, shuffle_placeholders, &mut rng
    ).expect("Failed to load/generate shuffle keys");

    let mut reveal_placeholders = Vec::new();
    for _ in 0..4 { reveal_placeholders.push(("Y".to_string(), 0.into())); }
    for _ in 0..2 { reveal_placeholders.push(("pkP".to_string(), 0.into())); }
    reveal_placeholders.push(("skP".to_string(), 0.into()));

    let (reveal_pk, reveal_vk) = load_or_generate_keys(
        reveal_r1cs, reveal_wasm, reveal_pk_cache, reveal_vk_cache, reveal_placeholders, &mut rng
    ).expect("Failed to load/generate reveal keys");

    let key_load_time = key_load_start.elapsed();
    println!("[DEBUG_LOG] Proving Keys ready. Time: {:.2}s", key_load_time.as_secs_f64());

    // 2. Generate key pairs for two users (Alice and Bob)
    let setup_start = Instant::now();
    let alice_keys = KeyPair::generate(&mut rng);
    let bob_keys = KeyPair::generate(&mut rng);

    // 3. Aggregate public key
    let aggregated_pk = (alice_keys.pk.into_group() + bob_keys.pk.into_group()).into_affine();

    // 4. Initialize deck (52 cards for the circuit)
    let g = Point::generator();
    let mut cards = Vec::new();
    for i in 1..=52 {
        let card_point = (g.into_group() * Fr::from(i as u64)).into_affine();
        cards.push(card_point);
    }

    let deck: Vec<Ciphertext> = cards.iter().map(|m| {
        let r = Fr::rand(&mut rng);
        encrypt(&aggregated_pk, m, &r)
    }).collect();

    // 5. Alice shuffles
    let mut deck = deck;
    let alice_shuffle = shuffle(&mut rng, &deck, &aggregated_pk);
    deck = alice_shuffle.deck;

    let setup_time = setup_start.elapsed();
    println!("[DEBUG_LOG] Setup complete (keys, deck encryption). Time: {:.2}s", setup_time.as_secs_f64());

    // Alice's Shuffle Proof Generation & Verification
    println!("[DEBUG_LOG] Generating Alice's Shuffle Proof...");
    let alice_shuffle_start = Instant::now();
    let alice_proof = match generate_shuffle_proof(
        shuffle_r1cs, shuffle_wasm, &shuffle_pk, &alice_shuffle.public_inputs, alice_shuffle.private_inputs, &mut rng
    ) {
        Ok(proof) => proof,
        Err(e) => {
            eprintln!("[ERROR] Failed to generate Alice's shuffle proof: {}", e);
            panic!("Proof generation failed");
        }
    };

    let alice_proof_gen_time = alice_shuffle_start.elapsed();
    println!("[DEBUG_LOG] Alice's Shuffle Proof generated successfully! Time: {:.2}s", alice_proof_gen_time.as_secs_f64());
    println!("[DEBUG_LOG] Proof has {} bytes when serialized", {
        use ark_serialize::CanonicalSerialize;
        let mut bytes = Vec::new();
        alice_proof.serialize_compressed(&mut bytes).unwrap();
        bytes.len()
    });

    println!("[DEBUG_LOG] Verifying Alice's Shuffle Proof...");
    let alice_verify_start = Instant::now();
    let public_inputs_vec = alice_shuffle.public_inputs.to_ark_public_inputs();
    println!("[DEBUG_LOG] Number of public inputs: {}", public_inputs_vec.len());
    println!("[DEBUG_LOG] VK expects {} public inputs (gamma_abc_g1.len() - 1)", shuffle_vk.gamma_abc_g1.len() - 1);

    // Try verification with just the raw proof verify API
    use ark_groth16::Groth16;
    use ark_snark::SNARK;
    use ark_bn254::Bn254;
    match Groth16::<Bn254>::verify(&shuffle_vk, &public_inputs_vec, &alice_proof) {
        Ok(true) => {
            let alice_verify_time = alice_verify_start.elapsed();
            println!("[DEBUG_LOG] Alice's Shuffle Proof verified successfully! Time: {:.2}s", alice_verify_time.as_secs_f64());
        },
        Ok(false) => panic!("[ERROR] Alice's Shuffle Proof verification returned false!"),
        Err(e) => {
            eprintln!("[ERROR] Alice's Shuffle Proof verification failed: {:?}", e);
            panic!("Verification failed");
        }
    }

    // 6. Bob shuffles
    let bob_shuffle = shuffle(&mut rng, &deck, &aggregated_pk);
    deck = bob_shuffle.deck;

    // Bob's Shuffle Proof Generation & Verification
    println!("[DEBUG_LOG] Generating Bob's Shuffle Proof...");
    let bob_shuffle_start = Instant::now();
    let bob_proof = generate_shuffle_proof(
        shuffle_r1cs, shuffle_wasm, &shuffle_pk, &bob_shuffle.public_inputs, bob_shuffle.private_inputs, &mut rng
    ).expect("Failed to generate Bob's shuffle proof");

    assert!(verify_shuffle_proof(&shuffle_vk, &bob_proof, &bob_shuffle.public_inputs).unwrap());
    let bob_shuffle_time = bob_shuffle_start.elapsed();
    println!("[DEBUG_LOG] Bob's Shuffle Proof verified. Time: {:.2}s", bob_shuffle_time.as_secs_f64());

    // 7. Reveal two cards
    let reveal_start = Instant::now();
    for card_index in 0..2 {
        let card_to_reveal = &deck[card_index];

        // Both parties provide partial decryptions
        let alice_reveal = reveal_card(&alice_keys.sk, card_to_reveal, &alice_keys.pk);
        let bob_reveal = reveal_card(&bob_keys.sk, card_to_reveal, &bob_keys.pk);

        // Verify Reveal ZK Proofs
        println!("[DEBUG_LOG] Generating Reveal Proofs for card {}...", card_index);
        let reveal_proof_start = Instant::now();
        let alice_reveal_proof = generate_reveal_proof(
            reveal_r1cs, reveal_wasm, &reveal_pk, &alice_reveal.public_inputs, alice_reveal.sk_p, &mut rng
        ).unwrap();
        let bob_reveal_proof = generate_reveal_proof(
            reveal_r1cs, reveal_wasm, &reveal_pk, &bob_reveal.public_inputs, bob_reveal.sk_p, &mut rng
        ).unwrap();

        let alice_verify = verify_reveal_proof(&reveal_vk, &alice_reveal_proof, &alice_reveal.public_inputs).unwrap();
        if !alice_verify {
            println!("[ERROR] Alice's reveal proof failed verification!");
            println!("[DEBUG] Public inputs count: {}", alice_reveal.public_inputs.to_ark_public_inputs().len());
            println!("[DEBUG] VK expects: {}", reveal_vk.gamma_abc_g1.len() - 1);
        }
        assert!(alice_verify, "Alice's reveal proof verification failed");

        let bob_verify = verify_reveal_proof(&reveal_vk, &bob_reveal_proof, &bob_reveal.public_inputs).unwrap();
        assert!(bob_verify, "Bob's reveal proof verification failed");
        let reveal_proof_time = reveal_proof_start.elapsed();
        println!("[DEBUG_LOG] Reveal Proofs for card {} verified. Time: {:.2}s", card_index, reveal_proof_time.as_secs_f64());

        // 8. Combine partial decryptions to get the card
        let combined_reveal = (alice_reveal.partial_decryption.into_group() + bob_reveal.partial_decryption.into_group()).into_affine();
        let revealed_card = (card_to_reveal.c1.into_group() - combined_reveal.into_group()).into_affine();

        // Logical verification with aggregated SK
        let aggregated_sk = alice_keys.sk + bob_keys.sk;
        let direct_decrypted = decrypt(&aggregated_sk, card_to_reveal);

        // 9. Verify the revealed card is one of the original cards
        let found = cards.iter().any(|&c| c == revealed_card);
        assert_eq!(revealed_card, direct_decrypted, "Reveal process mismatch for card {}!", card_index);
        assert!(found, "Revealed card {} was not in the original deck!", card_index);
    }

    let total_reveal_time = reveal_start.elapsed();
    let total_test_time = test_start.elapsed();

    println!("\n========== TIMING SUMMARY ==========");
    println!("Key Loading:              {:.2}s", key_load_time.as_secs_f64());
    println!("Setup (keys + encrypt):   {:.2}s", setup_time.as_secs_f64());
    println!("Alice Shuffle Proof Gen:  {:.2}s", alice_proof_gen_time.as_secs_f64());
    println!("Alice Shuffle Verify:     {:.2}s", alice_verify_start.elapsed().as_secs_f64());
    println!("Bob Shuffle (gen+verify): {:.2}s", bob_shuffle_time.as_secs_f64());
    println!("Reveal 2 cards (total):   {:.2}s", total_reveal_time.as_secs_f64());
    println!("------------------------------------");
    println!("TOTAL TEST TIME:          {:.2}s", total_test_time.as_secs_f64());
    println!("====================================\n");
}
