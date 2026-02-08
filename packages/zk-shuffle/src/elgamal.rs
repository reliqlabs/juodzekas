use crate::babyjubjub::{Point, Fr};
use ark_ec::{CurveGroup, AffineRepr};
use ark_ff::UniformRand;
use rand::Rng;

pub struct KeyPair {
    pub sk: Fr,
    pub pk: Point,
}

impl KeyPair {
    pub fn generate<R: Rng>(rng: &mut R) -> Self {
        let sk = Fr::rand(rng);
        let pk = (Point::generator() * sk).into_affine();
        KeyPair { sk, pk }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Ciphertext {
    pub c0: Point,
    pub c1: Point,
}

pub fn encrypt(pk: &Point, m: &Point, r: &Fr) -> Ciphertext {
    // c0 = r * g
    let c0 = (Point::generator() * *r).into_affine();
    // c1 = r * pk + m
    let c1 = (pk.into_group() * *r + m.into_group()).into_affine();
    Ciphertext { c0, c1 }
}

pub fn decrypt(sk: &Fr, c: &Ciphertext) -> Point {
    // m = c1 - sk * c0
    let sk_c0 = (c.c0.into_group() * *sk).into_affine();
    
    (c.c1.into_group() - sk_c0.into_group()).into_affine()
}
