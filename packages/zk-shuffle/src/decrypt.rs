use crate::babyjubjub::{Fr, Point};
use crate::elgamal::Ciphertext;
use crate::proof::RevealPublicInputs;
use ark_bn254::Fr as Bn254Fr;
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{BigInteger, PrimeField};

pub struct RevealResult {
    pub partial_decryption: Point,
    pub public_inputs: RevealPublicInputs,
    pub sk_p: Bn254Fr,
}

pub fn reveal_card(sk: &Fr, ciphertext: &Ciphertext, pk: &Point) -> RevealResult {
    // 1. Compute partial decryption: sk * c0
    // This is what we return and will be combined with other players' partial decryptions
    let partial_decryption = (ciphertext.c0.into_group() * *sk).into_affine();

    // 2. The decrypt circuit computes: out = c1 - sk * c0
    // In a multi-party setting, this is NOT the final message, but it's what the circuit outputs
    // We need to provide this as a public input for verification
    let circuit_output =
        (ciphertext.c1.into_group() - partial_decryption.into_group()).into_affine();

    // 3. Prepare ZK proof inputs
    let public_inputs = RevealPublicInputs::from_babyjubjub(
        [
            ciphertext.c0.x,
            ciphertext.c0.y,
            ciphertext.c1.x,
            ciphertext.c1.y,
        ],
        [pk.x, pk.y],
        [circuit_output.x, circuit_output.y], // The decryption output from the circuit
    );

    let sk_p = Bn254Fr::from_le_bytes_mod_order(&sk.into_bigint().to_bytes_le());

    RevealResult {
        partial_decryption,
        public_inputs,
        sk_p,
    }
}
