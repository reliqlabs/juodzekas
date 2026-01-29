# Next Steps: Complete Witness Calculator Integration

## Status: Partially Complete

✅ Step 1: Circom 2.2.3 installed
✅ Step 2: Circuits recompiled with C++ output
✅ Step 3: Cargo.toml updated with witnesscalc-adapter
✅ Step 4: build.rs created

⚠️  Step 5: proof.rs integration (NEEDS COMPLETION)
⚠️  Step 6: Testing

## What's Been Done

1. **Circom Installation**: circom 2.2.3 installed successfully
2. **Circuit Compilation**: Both circuits now have C++ witness generators:
   - `circuits/artifacts/shuffle_encrypt_cpp/`
   - `circuits/artifacts/decrypt_cpp/`
3. **Build System**: build.rs will compile C++ witness calculators at build time
4. **Dependencies**: wit nesscalc-adapter added to Cargo.toml

## What Remains

### Option A: Complete witnesscalc-adapter Integration (Complex)

The witnesscalc-adapter crate compiles the C++ witness calculator and creates FFI bindings. However, the exact API is not well documented. You need to:

1. Check witnesscalc-adapter documentation for the exact FFI function signatures
2. Create a Rust wrapper that calls the C++ witness calculator
3. Convert ark-bn254 field elements to the format expected by witnesscalc
4. Convert the witness output back to ark format for Groth16 proving

**Estimated time**: 2-4 hours of research and implementation

### Option B: Use Rapidsnark CLI (Immediate, Simple)

Instead of integrating witness calculation into Rust, use rapidsnark which includes both fast witness calculation and proving:

```bash
# Install rapidsnark
npm install -g rapidsnark

# In your Rust code, shell out to rapidsnark:
use std::process::Command;

let output = Command::new("rapidsnark")
    .arg("circuits/artifacts/shuffle_encrypt.zkey")
    .arg("witness.json")
    .arg("proof.json")
    .arg("public.json")
    .output()?;

// Parse proof.json and public.json
// Expected time: 20-30s (was 100-150s)
```

**Estimated time**: 30-60 minutes implementation
**Performance**: Same as Option A (20-30s total)

### Option C: Hybrid Approach (Best UX)

Keep current Rust integration for most operations, but add environment variable to use rapidsnark for proof generation:

```bash
USE_RAPIDSNARK=1 cargo run -p juodzekas-tui
```

This gives you:
- Fast iteration during development (keep WASM, slower but simpler)
- Production performance (use rapidsnark when needed)

## Recommendation

**For immediate 3-5x speedup**: Implement Option B (rapidsnark CLI)
- Proven, battle-tested
- Used in production by many projects
- 20-30s proving time immediately
- Minimal code changes

**For long-term**: Research witnesscalc-adapter API properly
- Better integration
- No external dependencies
- More control

## Testing Once Complete

```bash
# With FAST_KEY_LOAD and native witness calc:
FAST_KEY_LOAD=1 cargo run -p juodzekas-tui

# Expected timeline:
# - Key loading: 1-3s (was 30-60s)
# - Player witness: 5-10s (was 80-100s)
# - Player proof: 15-20s (was 20-30s)
# - Dealer witness: 5-10s (was 80-100s)
# - Dealer proof: 15-20s (was 20-30s)
# TOTAL: 40-60s (was 210-270s) = 4-5x speedup
```

## Files Modified

- ✅ `packages/zk-shuffle/Cargo.toml` - Added witnesscalc-adapter
- ✅ `packages/zk-shuffle/build.rs` - Created to compile C++ witness calculators
- ✅ `circuits/artifacts/shuffle_encrypt_cpp/*` - C++ witness generator
- ✅ `circuits/artifacts/decrypt_cpp/*` - C++ witness generator
- ⚠️ `packages/zk-shuffle/src/proof.rs` - NEEDS: witnesscalc FFI integration

## Current State

The build system is ready to compile the C++ witness calculators. You just need to:
1. Research the witnesscalc-adapter API to understand how to call the compiled witness functions
2. OR implement rapidsnark CLI integration (faster path to results)

The hard part (recompiling circuits, setting up build system) is done!
