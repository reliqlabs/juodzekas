# zk-shuffle

Mental Poker cryptographic library: ElGamal encryption over BabyJubJub, shuffle with re-encryption, partial decryption reveals, and Groth16 ZK proof generation via rapidsnark.

## What It Does

Implements the cryptographic core of a 2-party mental poker protocol:

1. **Key Generation** - ElGamal keypairs on BabyJubJub curve. Aggregated public key from both parties.
2. **Encryption** - Each card (a curve point) is encrypted under the aggregated public key.
3. **Shuffle** - Permute + re-encrypt the deck. Produces a ZK proof that the shuffle is valid (same cards, different ciphertexts).
4. **Reveal** - Each party computes a partial decryption of a card. Combined partials recover the plaintext. ZK proof ensures the partial is correctly computed from the party's secret key.

Card values: integers 0-51 mapped to BabyJubJub points (`generator * card_index`).

## API

```rust
use zk_shuffle::elgamal::{KeyPair, encrypt};
use zk_shuffle::shuffle::shuffle;
use zk_shuffle::decrypt::reveal_card;
use zk_shuffle::proof::{generate_shuffle_proof_rapidsnark, generate_reveal_proof_rapidsnark};

// Key generation
let keys = KeyPair::generate(&mut rng);
let aggregated_pk = (dealer_keys.pk + player_keys.pk).into_affine();

// Encrypt a deck
let ciphertext = encrypt(&aggregated_pk, &card_point, &randomness);

// Shuffle + re-encrypt
let result = shuffle(&mut rng, &encrypted_deck, &aggregated_pk);
// result.deck, result.public_inputs, result.private_inputs

// Generate shuffle ZK proof (requires tokio runtime context)
let proof = generate_shuffle_proof_rapidsnark(
    &result.public_inputs, result.private_inputs
).await?;

// Reveal a card (partial decryption)
let reveal = reveal_card(&my_sk, &ciphertext, &my_pk);
// reveal.partial_decryption, reveal.public_inputs

// Generate reveal ZK proof
let proof = generate_reveal_proof_rapidsnark(
    &reveal.public_inputs, reveal.sk_p
).await?;
```

## Circuit Artifacts

Proof generation requires compiled Circom artifacts at `circuits/circuit-artifacts/`:

| File | Size | Purpose |
|------|------|---------|
| `wasm/encrypt.wasm` | 1.2 MB | Shuffle witness calculator |
| `wasm/decrypt.wasm` | 88 KB | Reveal witness calculator |
| `zkey/encrypt.zkey` | 173 MB | Shuffle proving key |
| `zkey/decrypt.zkey` | 1.8 MB | Reveal proving key |

These are loaded via memory-mapped files at runtime. The WASM witness calculators run inside wasmer.

## Runtime Requirements

- **Tokio reactor context**: The WASM witness calculator (via wasmer/virtual-fs) requires a tokio reactor on the current thread. On bare `std::thread::spawn` threads, build a tokio runtime and call `let _guard = rt.enter()` before proof generation.
- **C++ toolchain**: cmake + GMP library (for rapidsnark native compilation).

### Installing C++ Dependencies

**macOS:**
```bash
brew install cmake gmp
```

**Ubuntu/Debian:**
```bash
sudo apt-get install cmake libgmp-dev build-essential
```

## Dependencies

- `ark-bn254`, `ark-ec`, `ark-ff`, `ark-std` 0.5 (finite field / curve arithmetic)
- `ark-groth16` 0.5, `ark-circom` 0.5 (Groth16 prover)
- `rust-rapidsnark` 0.1.3 (native Groth16 prover, ~10x faster than arkworks)
- `babyjubjub-rs` 0.0.11, `taceo-ark-babyjubjub` 0.5.3 (curve operations)
- `wasmer` 4.4 (WASM runtime for witness calculators)
- `memmap2` 0.9 (memory-mapped zkey files)

## Source Layout

```
src/
  lib.rs          Module exports, basic tests
  babyjubjub.rs   BabyJubJub type aliases (Point, Fr, Fq)
  elgamal.rs      ElGamal encryption (KeyPair, encrypt, Ciphertext)
  shuffle.rs      Shuffle algorithm (permute + re-encrypt)
  decrypt.rs      Partial decryption (reveal_card)
  proof.rs        ZK proof generation (rapidsnark + WASM witness calc)
  error.rs        Error types
```
