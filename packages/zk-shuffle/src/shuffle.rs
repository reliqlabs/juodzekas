use crate::babyjubjub::{is_y_negative, Fq, Fr, Point};
use crate::elgamal::{encrypt, Ciphertext};
use crate::proof::ShufflePublicInputs;
use ark_bn254::Fr as Bn254Fr;
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{BigInteger, PrimeField};
use ark_std::UniformRand;
use rand::seq::SliceRandom;
use rand::Rng;

pub struct ShuffleResult {
    pub deck: Vec<Ciphertext>,
    pub public_inputs: ShufflePublicInputs,
    pub private_inputs: Vec<(String, Vec<Bn254Fr>)>,
}

pub fn shuffle<R: Rng>(rng: &mut R, deck: &[Ciphertext], aggregated_pk: &Point) -> ShuffleResult {
    // 1. Permute deck
    let mut indices: Vec<usize> = (0..deck.len()).collect();
    indices.shuffle(rng);

    let shuffled_deck: Vec<Ciphertext> = indices.iter().map(|&i| deck[i].clone()).collect();

    // 2. Re-encrypt each card (homomorphic property)
    let zero_point = Point::default();
    let mut r_primes = Vec::new();
    let re_encrypted_deck: Vec<Ciphertext> = shuffled_deck
        .iter()
        .map(|c| {
            let r_prime = Fr::rand(rng);
            r_primes.push(r_prime);
            let delta = encrypt(aggregated_pk, &zero_point, &r_prime);
            Ciphertext {
                c0: (c.c0.into_group() + delta.c0.into_group()).into_affine(),
                c1: (c.c1.into_group() + delta.c1.into_group()).into_affine(),
            }
        })
        .collect();

    // 3. Prepare ZK proof inputs
    let convert_fr = |f: &Fr| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le());

    let ux0: Vec<Fq> = deck.iter().map(|c| c.c0.x).collect();
    let ux1: Vec<Fq> = deck.iter().map(|c| c.c1.x).collect();
    let vx0_prime: Vec<Fq> = re_encrypted_deck.iter().map(|c| c.c0.x).collect();
    let vx1_prime: Vec<Fq> = re_encrypted_deck.iter().map(|c| c.c1.x).collect();

    let mut s_u0 = Bn254Fr::from(0);
    let mut s_u1 = Bn254Fr::from(0);
    let mut s_v0 = Bn254Fr::from(0);
    let mut s_v1 = Bn254Fr::from(0);

    // The circuit expects:
    // - delta to be the canonical (positive) representation of y-coordinate (< (q-1)/2)
    // - s bit indicates if we need to use the negative: if s=0, y=-delta; if s=1, y=delta
    let u_delta0: Vec<Bn254Fr> = deck
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let y_full = c.c0.y;
            let is_neg = is_y_negative(y_full);
            // s=0 means use negative (the circuit does (s-1)*delta when s=0)
            // s=1 means use positive (the circuit does s*delta when s=1)
            // But wait - the circuit does: y = s*delta + (s-1)*delta
            // When s=1: y = 1*delta + 0*delta = delta
            // When s=0: y = 0*delta + (-1)*delta = -delta
            // So: s=1 for positive, s=0 for negative
            // But our is_neg means "y is in the upper half", which represents a negative value
            // So when is_neg=true, we want s=0 (to get -delta)
            // Therefore: s = !is_neg
            if !is_neg {
                s_u0 += Bn254Fr::from(1u128 << i);
            }
            let delta = if is_neg { -y_full } else { y_full };
            Bn254Fr::from_le_bytes_mod_order(&delta.into_bigint().to_bytes_le())
        })
        .collect();

    let u_delta1: Vec<Bn254Fr> = deck
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let y_full = c.c1.y;
            let is_neg = is_y_negative(y_full);
            if !is_neg {
                s_u1 += Bn254Fr::from(1u128 << i);
            }
            let delta = if is_neg { -y_full } else { y_full };
            Bn254Fr::from_le_bytes_mod_order(&delta.into_bigint().to_bytes_le())
        })
        .collect();

    let v_delta0: Vec<Bn254Fr> = re_encrypted_deck
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let y_full = c.c0.y;
            let is_neg = is_y_negative(y_full);
            if !is_neg {
                s_v0 += Bn254Fr::from(1u128 << i);
            }
            let delta = if is_neg { -y_full } else { y_full };
            Bn254Fr::from_le_bytes_mod_order(&delta.into_bigint().to_bytes_le())
        })
        .collect();

    let v_delta1: Vec<Bn254Fr> = re_encrypted_deck
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let y_full = c.c1.y;
            let is_neg = is_y_negative(y_full);
            if !is_neg {
                s_v1 += Bn254Fr::from(1u128 << i);
            }
            let delta = if is_neg { -y_full } else { y_full };
            Bn254Fr::from_le_bytes_mod_order(&delta.into_bigint().to_bytes_le())
        })
        .collect();

    let public_inputs = ShufflePublicInputs {
        pk: [
            Bn254Fr::from_le_bytes_mod_order(&aggregated_pk.x.into_bigint().to_bytes_le()),
            Bn254Fr::from_le_bytes_mod_order(&aggregated_pk.y.into_bigint().to_bytes_le()),
        ],
        ux0: ux0
            .iter()
            .map(|f| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le()))
            .collect(),
        ux1: ux1
            .iter()
            .map(|f| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le()))
            .collect(),
        vx0: vx0_prime
            .iter()
            .map(|f| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le()))
            .collect(),
        vx1: vx1_prime
            .iter()
            .map(|f| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le()))
            .collect(),
        s_u: [s_u0, s_u1],
        s_v: [s_v0, s_v1],
    };

    // Permutation matrix A
    let mut a_matrix = vec![Bn254Fr::from(0); deck.len() * deck.len()];
    for (i, &shuffle_idx) in indices.iter().enumerate() {
        // shuffled_deck[i] = deck[shuffle_idx]
        // B[i] = Sum_j A[i][j] * X[j]
        // So B[i] = X[shuffle_idx] implies A[i][shuffle_idx] = 1
        a_matrix[i * deck.len() + shuffle_idx] = Bn254Fr::from(1);
    }

    let private_inputs = vec![
        ("R".to_string(), r_primes.iter().map(convert_fr).collect()),
        ("A".to_string(), a_matrix),
        ("UDelta0".to_string(), u_delta0),
        ("UDelta1".to_string(), u_delta1),
        ("VDelta0".to_string(), v_delta0),
        ("VDelta1".to_string(), v_delta1),
    ];

    ShuffleResult {
        deck: re_encrypted_deck,
        public_inputs,
        private_inputs,
    }
}
