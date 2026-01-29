use zk_shuffle::elgamal::{KeyPair, encrypt, decrypt, Ciphertext};
use zk_shuffle::shuffle::shuffle;
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::babyjubjub::{Point, Fr};
use ark_std::UniformRand;
use ark_ec::{CurveGroup, AffineRepr};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::time::Instant;

use zk_shuffle::proof::{
    generate_shuffle_proof_rapidsnark, verify_shuffle_proof_rapidsnark,
    generate_reveal_proof_rapidsnark, verify_reveal_proof_rapidsnark,
};

#[tokio::test]
async fn test_rapidsnark_shuffle_and_reveal() {
    let test_start = Instant::now();

    // 1. Setup with a fixed seed for reproducibility
    let mut rng = ChaCha8Rng::seed_from_u64(1337);

    // Paths to zkey and vkey files (rapidsnark format)
    let shuffle_vkey = "../../circuits/artifacts/shuffle_encrypt_vkey.json";
    let reveal_vkey = "../../circuits/artifacts/decrypt_vkey.json";

    println!("[TEST] Using rapidsnark/witnesscalc for proof generation");
    println!("[TEST] Shuffle vkey: {}", shuffle_vkey);
    println!("[TEST] Reveal vkey: {}", reveal_vkey);

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
    println!("[TEST] Setup complete (keys, deck encryption). Time: {:.2}s", setup_time.as_secs_f64());

    // Alice's Shuffle Proof Generation & Verification (using rapidsnark)
    println!("[TEST] Generating Alice's Shuffle Proof with rapidsnark...");
    let alice_shuffle_start = Instant::now();
    let alice_proof = match generate_shuffle_proof_rapidsnark(
        &alice_shuffle.public_inputs,
        alice_shuffle.private_inputs,
    ) {
        Ok(proof) => proof,
        Err(e) => {
            eprintln!("[ERROR] Failed to generate Alice's shuffle proof: {}", e);
            panic!("Proof generation failed");
        }
    };

    let alice_proof_gen_time = alice_shuffle_start.elapsed();
    println!("[TEST] Alice's Shuffle Proof generated successfully! Time: {:.2}s", alice_proof_gen_time.as_secs_f64());

    println!("[TEST] Verifying Alice's Shuffle Proof...");
    let alice_verify_start = Instant::now();
    match verify_shuffle_proof_rapidsnark(shuffle_vkey, &alice_proof, &alice_shuffle.public_inputs) {
        Ok(true) => {
            let alice_verify_time = alice_verify_start.elapsed();
            println!("[TEST] Alice's Shuffle Proof verified successfully! Time: {:.2}s", alice_verify_time.as_secs_f64());
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
    println!("[TEST] Generating Bob's Shuffle Proof with rapidsnark...");
    let bob_shuffle_start = Instant::now();
    let bob_proof = generate_shuffle_proof_rapidsnark(
        &bob_shuffle.public_inputs,
        bob_shuffle.private_inputs,
    ).expect("Failed to generate Bob's shuffle proof");

    assert!(verify_shuffle_proof_rapidsnark(shuffle_vkey, &bob_proof, &bob_shuffle.public_inputs).unwrap());
    let bob_shuffle_time = bob_shuffle_start.elapsed();
    println!("[TEST] Bob's Shuffle Proof verified. Time: {:.2}s", bob_shuffle_time.as_secs_f64());

    // 7. Reveal two cards with rapidsnark proofs
    let reveal_start = Instant::now();
    for card_index in 0..2 {
        let card_to_reveal = &deck[card_index];

        // Both parties provide partial decryptions
        let alice_reveal = reveal_card(&alice_keys.sk, card_to_reveal, &alice_keys.pk);
        let bob_reveal = reveal_card(&bob_keys.sk, card_to_reveal, &bob_keys.pk);

        // Verify Reveal ZK Proofs (using rapidsnark)
        println!("[TEST] Generating Reveal Proofs for card {} with rapidsnark...", card_index);
        let reveal_proof_start = Instant::now();
        let alice_reveal_proof = generate_reveal_proof_rapidsnark(
            &alice_reveal.public_inputs,
            alice_reveal.sk_p,
        ).unwrap();
        let bob_reveal_proof = generate_reveal_proof_rapidsnark(
            &bob_reveal.public_inputs,
            bob_reveal.sk_p,
        ).unwrap();

        let alice_verify = verify_reveal_proof_rapidsnark(reveal_vkey, &alice_reveal_proof, &alice_reveal.public_inputs).unwrap();
        if !alice_verify {
            println!("[ERROR] Alice's reveal proof failed verification!");
        }
        assert!(alice_verify, "Alice's reveal proof verification failed");

        let bob_verify = verify_reveal_proof_rapidsnark(reveal_vkey, &bob_reveal_proof, &bob_reveal.public_inputs).unwrap();
        assert!(bob_verify, "Bob's reveal proof verification failed");
        let reveal_proof_time = reveal_proof_start.elapsed();
        println!("[TEST] Reveal Proofs for card {} verified. Time: {:.2}s", card_index, reveal_proof_time.as_secs_f64());

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

    println!("\n========== RAPIDSNARK TIMING SUMMARY ==========");
    println!("Setup (keys + encrypt):   {:.2}s", setup_time.as_secs_f64());
    println!("Alice Shuffle Proof Gen:  {:.2}s", alice_proof_gen_time.as_secs_f64());
    println!("Alice Shuffle Verify:     {:.2}s", alice_verify_start.elapsed().as_secs_f64());
    println!("Bob Shuffle (gen+verify): {:.2}s", bob_shuffle_time.as_secs_f64());
    println!("Reveal 2 cards (total):   {:.2}s", total_reveal_time.as_secs_f64());
    println!("-----------------------------------------------");
    println!("TOTAL TEST TIME:          {:.2}s", total_test_time.as_secs_f64());
    println!("===============================================\n");
}
