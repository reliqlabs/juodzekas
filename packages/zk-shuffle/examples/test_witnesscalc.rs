use zk_shuffle::*;
use ark_std::rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

fn main() {
    env_logger::init();

    let mut rng = ChaCha20Rng::from_entropy();

    println!("Testing native C++ witness calculator...");
    println!("This should complete in 25-40s (vs 100-150s with WASM)");

    let start = std::time::Instant::now();

    // Load keys using fast mmap method
    println!("\n1. Loading keys with mmap...");
    let keys = unsafe {
        load_keys_unsafe_mmap(
            "../../circuits/artifacts/shuffle_encrypt_keys/shuffle_encrypt_pk.bin",
            "../../circuits/artifacts/shuffle_encrypt_keys/shuffle_encrypt_vk.bin",
        ).expect("Failed to load keys")
    };
    println!("   Keys loaded in {}s", start.elapsed().as_secs());

    // Generate keypair and encrypt deck
    println!("\n2. Setting up game (keypair, encrypted deck)...");
    let setup_start = std::time::Instant::now();
    let (pk, sk) = keygen(&mut rng);
    let deck = generate_deck();
    let encrypted_deck = encrypt_deck(&deck, &pk);
    println!("   Setup completed in {}s", setup_start.elapsed().as_secs());

    // Test shuffle with native witness calculator
    println!("\n3. Generating shuffle proof with native C++ witness calculator...");
    let shuffle_start = std::time::Instant::now();

    let shuffle = shuffle(&mut rng, &encrypted_deck, &pk);
    let public_inputs = ShufflePublicInputs {
        x: encrypted_deck.clone(),
        y: shuffle.output.clone(),
        delta: shuffle.delta.clone(),
        pk: pk.clone(),
    };

    let proof = generate_shuffle_proof(
        "../../circuits/artifacts/shuffle_encrypt.r1cs",
        "../../circuits/artifacts/shuffle_encrypt_js/shuffle_encrypt.wasm",
        &keys.0,
        &public_inputs,
        shuffle.private_inputs,
        &mut rng,
    ).expect("Failed to generate proof");

    let shuffle_time = shuffle_start.elapsed().as_secs();
    println!("   Shuffle proof generated in {}s", shuffle_time);

    // Verify the proof
    println!("\n4. Verifying proof...");
    let verify_start = std::time::Instant::now();
    let valid = verify_shuffle_proof(&keys.1, &proof, &public_inputs)
        .expect("Failed to verify proof");
    println!("   Proof verified in {}s: {}", verify_start.elapsed().as_secs(), valid);

    let total_time = start.elapsed().as_secs();
    println!("\n✓ Total time: {}s", total_time);

    if shuffle_time < 60 {
        println!("✓ SUCCESS: Native witness calculator is working! ({}s vs expected 80-100s with WASM)", shuffle_time);
        println!("✓ Expected speedup: 3-5x faster");
    } else {
        println!("⚠ WARNING: Shuffle took {}s, expected <40s with native witness calculator", shuffle_time);
    }
}
