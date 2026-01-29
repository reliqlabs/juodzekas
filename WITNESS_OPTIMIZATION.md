# Witness Generation Optimization Plan

## Current Performance
- **Witness generation (WASM)**: ~80-100s
- **Proof generation (Groth16)**: ~20-30s
- **Total**: 100-150s per shuffle

## Target Performance
- **Witness generation (Native C++)**: ~5-10s
- **Proof generation (Groth16)**: ~20-30s
- **Total**: 25-40s per shuffle (3-5x faster)

## Implementation Steps

### Step 1: Install Circom (if not already installed)

```bash
# Install circom v2.0+
cargo install circom@2.1.8

# Or using npm
npm install -g circom@latest
```

### Step 2: Recompile Circuits with C++ Output

```bash
cd circuits

# Compile shuffle_encrypt circuit with C++ witness generator
circom shuffle_encrypt/shuffle_encrypt.circom \
  --r1cs --wasm --c --sym \
  --O2 \
  --output artifacts

# Compile decrypt circuit with C++ witness generator
circom decrypt/decrypt.circom \
  --r1cs --wasm --c --sym \
  --O2 \
  --output artifacts
```

This will generate:
- `artifacts/shuffle_encrypt_cpp/` - C++ witness generator
  - `shuffle_encrypt.cpp`
  - `shuffle_encrypt.dat`
  - `Makefile`
- `artifacts/decrypt_cpp/` - C++ witness generator
  - `decrypt.cpp`
  - `decrypt.dat`
  - `Makefile`

### Step 3: Update zk-shuffle/Cargo.toml

```toml
[dependencies]
ark-bn254 = { version = "0.5.0", features = ["parallel"] }
ark-ec = { version = "0.5.0", features = ["parallel"] }
ark-ff = { version = "0.5.0", features = ["parallel"] }
ark-std = { version = "0.5.0", features = ["parallel"] }
ark-groth16 = { version = "0.5.0", features = ["parallel"] }
ark-relations = "0.5.0"
ark-snark = "0.5.0"
# Remove: ark-circom = "0.5.0"
# Add:
circom-prover = { version = "0.2", default-features = false, features = ["witnesscalc"] }
witnesscalc-adapter = "0.2"
# ... rest of dependencies

[build-dependencies]
witnesscalc-adapter = "0.2"
```

### Step 4: Create packages/zk-shuffle/build.rs

```rust
use witnesscalc_adapter;

fn main() {
    // Compile shuffle_encrypt C++ witness generator
    witnesscalc_adapter::build_and_link("../../circuits/artifacts/shuffle_encrypt_cpp")
        .expect("Failed to compile shuffle_encrypt witness generator");

    // Compile decrypt C++ witness generator
    witnesscalc_adapter::build_and_link("../../circuits/artifacts/decrypt_cpp")
        .expect("Failed to compile decrypt witness generator");
}
```

### Step 5: Update packages/zk-shuffle/src/proof.rs

Replace the WASM-based witness generation with native C++ calls.

**Current approach:**
```rust
let cfg = CircomConfig::<Bn254Fr>::new(wasm_path, r1cs_path)?;
let mut builder = CircomBuilder::new(cfg);
// ... push inputs
let circom = builder.build()?; // ← SLOW: 80-100s for 454k constraints
```

**New approach:**
```rust
use circom_prover::{WitnessCalculator, witness};
use std::collections::HashMap;

// Prepare inputs as JSON
let mut inputs = HashMap::new();
for (name, vals) in public_inputs.get_input_mapping() {
    let str_vals: Vec<String> = vals.iter()
        .map(|v| v.to_string())
        .collect();
    inputs.insert(name, str_vals);
}
// Add private inputs
for (name, vals) in private_inputs {
    let str_vals: Vec<String> = vals.iter()
        .map(|v| v.to_string())
        .collect();
    inputs.insert(name, str_vals);
}

// Generate witness using native C++ (FAST: 5-10s)
extern "C" {
    fn shuffle_encrypt_witness(
        input_json: *const std::os::raw::c_char,
        witness_buffer: *mut u8,
        witness_size: *mut u32,
    ) -> i32;
}

let input_json = serde_json::to_string(&inputs)?;
// Call native witness calculator...
// Convert witness to ark format and create proof
```

### Step 6: Test Performance

```bash
# Run integration test
cargo test -p zk-shuffle test_52_card_shuffle_with_mmap_keys -- --nocapture

# Expected output:
# Player proof generation took 8s   (was ~95s)
# Dealer proof generation took 8s   (was ~95s)
# Total: ~25-35s (was ~100-150s)
```

## Alternative: Use Rapidsnark

If the above integration is too complex, use rapidsnark CLI:

```bash
# Install rapidsnark
npm install -g rapidsnark

# Generate proof (includes fast witness calc + proving)
rapidsnark circuits/artifacts/shuffle_encrypt.zkey witness.json proof.json public.json
```

This gives you 20-30s performance immediately without code changes.

## Notes

- C++ witness generation requires C++ compiler (clang/gcc)
- The `.cpp`/`.dat` files are platform-specific compiled artifacts
- Performance gain is 10-25x for witness generation
- Total end-to-end improvement: 3-5x (100-150s → 25-40s)
