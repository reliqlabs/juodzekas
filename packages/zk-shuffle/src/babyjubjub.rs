use ark_ff::{BigInteger, PrimeField};
use num_bigint::BigUint;
pub use taceo_ark_babyjubjub::EdwardsConfig;
pub use taceo_ark_babyjubjub::Fq;
pub use taceo_ark_babyjubjub::Fr;

pub type Point = ark_ec::twisted_edwards::Affine<EdwardsConfig>;

pub fn get_q() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .unwrap()
}

pub fn is_y_negative(y: Fq) -> bool {
    let q = get_q();
    let y_big = BigUint::from_bytes_le(&y.into_bigint().to_bytes_le());
    y_big * 2u32 > q
}
