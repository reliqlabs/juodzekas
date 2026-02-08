pub mod babyjubjub;
pub mod elgamal;
pub mod shuffle;
pub mod decrypt;
pub mod error;
pub mod proof;

pub use error::Error;

#[cfg(test)]
mod tests {
    use crate::elgamal::{KeyPair, encrypt, decrypt};
    use crate::shuffle::shuffle;
    use crate::decrypt::reveal_card;
    use crate::proof::ShufflePublicInputs;
    use crate::babyjubjub::{Point, Fr, Fq};
    use ark_std::UniformRand;
    use ark_ec::{CurveGroup, AffineRepr};

    #[test]
    fn test_elgamal_encryption_decryption() {
        let mut rng = ark_std::test_rng();
        let keypair = KeyPair::generate(&mut rng);
        
        // Use generator as a message
        let m = Point::generator();
        let r = Fr::rand(&mut rng);
        
        let c = encrypt(&keypair.pk, &m, &r);
        let decrypted_m = decrypt(&keypair.sk, &c);
        
        assert_eq!(m, decrypted_m);
    }

    #[test]
    fn test_shuffle_and_reveal() {
        let mut rng = ark_std::test_rng();
        let keypair1 = KeyPair::generate(&mut rng);
        let keypair2 = KeyPair::generate(&mut rng);
        
        // Aggregated public key (homomorphic addition)
        let aggregated_pk = (keypair1.pk.into_group() + keypair2.pk.into_group()).into_affine();
        
        // Initial deck: 3 cards (represented by points)
        let g = Point::generator();
        let cards = [g, (g.into_group() + g.into_group()).into_affine(), (g.into_group() + g.into_group() + g.into_group()).into_affine()];
        
        let mut deck: Vec<crate::elgamal::Ciphertext> = cards.iter().map(|m| {
            let r = Fr::rand(&mut rng);
            encrypt(&aggregated_pk, m, &r)
        }).collect();
        
        // Player 1 shuffles
        let result1 = shuffle(&mut rng, &deck, &aggregated_pk);
        deck = result1.deck;
        
        // Player 2 shuffles
        let result2 = shuffle(&mut rng, &deck, &aggregated_pk);
        deck = result2.deck;
        
        // Reveal first card
        let c = &deck[0];
        let reveal1 = reveal_card(&keypair1.sk, c, &keypair1.pk);
        let reveal2 = reveal_card(&keypair2.sk, c, &keypair2.pk);
        
        // Combine partial decryptions to get the card
        // m = c1 - (reveal1 + reveal2)
        let combined_reveal = (reveal1.partial_decryption.into_group() + reveal2.partial_decryption.into_group()).into_affine();
        let revealed_card = (c.c1.into_group() - combined_reveal.into_group()).into_affine();
        
        // The revealed card should be one of the original cards
        assert!(cards.contains(&revealed_card));
    }

    #[test]
    fn test_proof_verification_logic() {
        let mut rng = ark_std::test_rng();
        
        let pk = [Fq::rand(&mut rng), Fq::rand(&mut rng)];
        let ux0 = vec![Fq::rand(&mut rng); 52];
        let ux1 = vec![Fq::rand(&mut rng); 52];
        let vx0 = vec![Fq::rand(&mut rng); 52];
        let vx1 = vec![Fq::rand(&mut rng); 52];
        let s_u = [Fq::rand(&mut rng), Fq::rand(&mut rng)];
        let s_v = [Fq::rand(&mut rng), Fq::rand(&mut rng)];

        let public_inputs = ShufflePublicInputs::from_babyjubjub(
            pk, ux0, ux1, vx0, vx1, s_u, s_v
        );
        
        let ark_public_inputs = public_inputs.to_ark_public_inputs();
        assert_eq!(ark_public_inputs.len(), 1 + 2 + 52 * 4 + 2 + 2);
    }
}
