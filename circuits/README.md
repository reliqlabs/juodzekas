# Circuits

This directory contains the Circom circuits used for the trustless shuffling and card reveal logic in Juodžekas.

## Directory Structure

- `common/`: Shared templates for BabyJubJub, ElGamal, and matrix operations.
- `shuffle_encrypt/`: Circuits for shuffling and re-encrypting the deck.
- `decrypt/`: Circuits for partially decrypting cards for reveal.
- `tests/`: Test circuits for individual components.

## Dependencies

The circuits depend on [circomlib](https://github.com/iden3/circomlib), which is included in the `vendor/` directory at the project root.

## Compiling Circuits

To compile the circuits, you need `circom` (v2.0.0 or higher) installed.

Example for compiling the shuffle circuit:

```bash
circom -o . --r1cs --wasm --sym circuits/shuffle_encrypt/shuffle_encrypt.circom
```

## Running Tests

The original circuits used `hardhat` and `circom_tester`. You can run tests if you have a Node.js environment set up with these dependencies.

In a future update, we plan to integrate these circuits directly into the Rust test suite using `ark-circom`.

## Provenance

These circuits are sourced from [burnt-labs/zkShuffle](https://github.com/burnt-labs/zkShuffle).
