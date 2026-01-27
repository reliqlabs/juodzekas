use crate::babyjubjub::Fq;
use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_groth16::{Groth16, Proof, VerifyingKey, ProvingKey};
use ark_snark::{SNARK, CircuitSpecificSetupSNARK};
use ark_relations::r1cs::SynthesisError;
use ark_ff::{PrimeField, BigInteger};
use ark_circom::{CircomBuilder, CircomConfig};
use ark_std::rand::{Rng, CryptoRng};
use std::fs::File;
pub use ark_serialize::{CanonicalSerialize, CanonicalDeserialize};

pub fn load_or_generate_keys<R: Rng + CryptoRng>(
    r1cs_path: &str,
    wasm_path: &str,
    pk_cache_path: &str,
    vk_cache_path: &str,
    input_placeholders: Vec<(String, num_bigint::BigInt)>,
    rng: &mut R,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>), Box<dyn std::error::Error>> {
    // Try to load from cache if both files exist
    if std::path::Path::new(pk_cache_path).exists() && std::path::Path::new(vk_cache_path).exists() {
        println!("Loading cached keys from {} and {}", pk_cache_path, vk_cache_path);
        match (|| -> Result<_, Box<dyn std::error::Error>> {
            let pk_file = File::open(pk_cache_path)?;
            let vk_file = File::open(vk_cache_path)?;

            println!("Deserializing proving key (this may take 30-60 seconds for large circuits)...");
            let pk = ProvingKey::<Bn254>::deserialize_uncompressed_unchecked(pk_file)?;
            println!("Deserializing verifying key...");
            let vk = VerifyingKey::<Bn254>::deserialize_uncompressed_unchecked(vk_file)?;

            Ok((pk, vk))
        })() {
            Ok((pk, vk)) => {
                println!("Successfully loaded cached keys");
                return Ok((pk, vk));
            }
            Err(e) => {
                println!("Failed to load cached keys: {}. Regenerating...", e);
                // Fall through to regenerate keys
            }
        }
    }

    println!("Generating new keys (this may take several minutes for large circuits)...");
    let cfg = CircomConfig::<Bn254Fr>::new(wasm_path, r1cs_path)?;
    let mut builder = CircomBuilder::new(cfg);
    for (name, val) in input_placeholders {
        builder.push_input(&name, val);
    }
    let circuit = builder.setup();
    let (pk, vk) = Groth16::<Bn254>::setup(circuit, rng)?;

    // Ensure cache directory exists before writing
    if let Some(parent) = std::path::Path::new(pk_cache_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = std::path::Path::new(vk_cache_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    println!("Caching keys to {} and {} (uncompressed for faster loading)...", pk_cache_path, vk_cache_path);
    let mut pk_file = File::create(pk_cache_path)?;
    let mut vk_file = File::create(vk_cache_path)?;
    pk.serialize_uncompressed(&mut pk_file)?;
    vk.serialize_uncompressed(&mut vk_file)?;
    println!("Successfully cached keys");

    Ok((pk, vk))
}

#[derive(Clone, Debug)]
pub struct ShufflePublicInputs {
    pub pk: [Bn254Fr; 2],
    pub ux0: Vec<Bn254Fr>,
    pub ux1: Vec<Bn254Fr>,
    pub vx0: Vec<Bn254Fr>,
    pub vx1: Vec<Bn254Fr>,
    pub s_u: [Bn254Fr; 2],
    pub s_v: [Bn254Fr; 2],
}

impl ShufflePublicInputs {
    pub fn from_babyjubjub(
        pk: [Fq; 2],
        ux0: Vec<Fq>,
        ux1: Vec<Fq>,
        vx0: Vec<Fq>,
        vx1: Vec<Fq>,
        s_u: [Fq; 2],
        s_v: [Fq; 2],
    ) -> Self {
        let convert = |f: &Fq| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le());
        Self {
            pk: [convert(&pk[0]), convert(&pk[1])],
            ux0: ux0.iter().map(convert).collect(),
            ux1: ux1.iter().map(convert).collect(),
            vx0: vx0.iter().map(convert).collect(),
            vx1: vx1.iter().map(convert).collect(),
            s_u: [convert(&s_u[0]), convert(&s_u[1])],
            s_v: [convert(&s_v[0]), convert(&s_v[1])],
        }
    }

    pub fn to_ark_public_inputs(&self) -> Vec<Bn254Fr> {
        let mut inputs = Vec::new();
        // Circuit output comes first (dummy_output = pk[0] * pk[1])
        let dummy_output = self.pk[0] * self.pk[1];
        inputs.push(dummy_output);
        // Then all the declared public inputs
        for f in &self.pk { inputs.push(*f); }
        for f in &self.ux0 { inputs.push(*f); }
        for f in &self.ux1 { inputs.push(*f); }
        for f in &self.vx0 { inputs.push(*f); }
        for f in &self.vx1 { inputs.push(*f); }
        for f in &self.s_u { inputs.push(*f); }
        for f in &self.s_v { inputs.push(*f); }
        inputs
    }

    pub fn get_input_mapping(&self) -> Vec<(String, Vec<Bn254Fr>)> {
        let mut mapping = Vec::new();
        mapping.push(("pk".to_string(), self.pk.to_vec()));
        mapping.push(("UX0".to_string(), self.ux0.clone()));
        mapping.push(("UX1".to_string(), self.ux1.clone()));
        mapping.push(("VX0".to_string(), self.vx0.clone()));
        mapping.push(("VX1".to_string(), self.vx1.clone()));
        mapping.push(("s_u".to_string(), self.s_u.to_vec()));
        mapping.push(("s_v".to_string(), self.s_v.to_vec()));
        mapping
    }
}

/// Generates a ZK proof for the shuffle operation.
/// This requires the compiled R1CS and WASM witness generator.
pub fn generate_shuffle_proof<R: Rng + CryptoRng>(
    r1cs_path: &str,
    wasm_path: &str,
    pk: &ProvingKey<Bn254>,
    public_inputs: &ShufflePublicInputs,
    private_inputs: Vec<(String, Vec<Bn254Fr>)>,
    rng: &mut R,
) -> Result<Proof<Bn254>, Box<dyn std::error::Error>> {
    let cfg = CircomConfig::<Bn254Fr>::new(wasm_path, r1cs_path)?;
    let mut builder = CircomBuilder::new(cfg);
    
    let convert_to_bigint = |f: &Bn254Fr| num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le());

    for (name, vals) in public_inputs.get_input_mapping() {
        for val in vals {
            builder.push_input(&name, convert_to_bigint(&val));
        }
    }

    // Add private inputs (A, R, UDelta0, etc.)
    for (name, vals) in private_inputs {
        for val in vals {
            builder.push_input(&name, convert_to_bigint(&val));
        }
    }

    let circom = builder.build()?;
    let proof = Groth16::<Bn254>::prove(pk, circom, rng)?;
    Ok(proof)
}

#[derive(Clone, Debug)]
pub struct RevealPublicInputs {
    pub y: [Bn254Fr; 4],
    pub pk_p: [Bn254Fr; 2],
    pub out: [Bn254Fr; 2],  // The decryption output (partial decryption)
}

impl RevealPublicInputs {
    pub fn from_babyjubjub(
        y: [Fq; 4],
        pk_p: [Fq; 2],
        out: [Fq; 2],
    ) -> Self {
        let convert = |f: &Fq| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le());
        Self {
            y: [convert(&y[0]), convert(&y[1]), convert(&y[2]), convert(&y[3])],
            pk_p: [convert(&pk_p[0]), convert(&pk_p[1])],
            out: [convert(&out[0]), convert(&out[1])],
        }
    }

    pub fn to_ark_public_inputs(&self) -> Vec<Bn254Fr> {
        let mut inputs = Vec::new();
        // Circuit outputs come first
        for f in &self.out { inputs.push(*f); }
        // Then the declared public inputs
        for f in &self.y { inputs.push(*f); }
        for f in &self.pk_p { inputs.push(*f); }
        inputs
    }
}

/// Generates a ZK proof for the reveal operation.
pub fn generate_reveal_proof<R: Rng + CryptoRng>(
    r1cs_path: &str,
    wasm_path: &str,
    pk: &ProvingKey<Bn254>,
    public_inputs: &RevealPublicInputs,
    sk_p: Bn254Fr,
    rng: &mut R,
) -> Result<Proof<Bn254>, Box<dyn std::error::Error>> {
    let cfg = CircomConfig::<Bn254Fr>::new(wasm_path, r1cs_path)?;
    let mut builder = CircomBuilder::new(cfg);
    
    let convert_to_bigint = |f: &Bn254Fr| num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le());

    for val in &public_inputs.y { builder.push_input("Y", convert_to_bigint(val)); }
    for val in &public_inputs.pk_p { builder.push_input("pkP", convert_to_bigint(val)); }
    builder.push_input("skP", convert_to_bigint(&sk_p));

    let circom = builder.build()?;
    let proof = Groth16::<Bn254>::prove(pk, circom, rng)?;
    Ok(proof)
}

pub fn verify_reveal_proof(
    vk: &VerifyingKey<Bn254>,
    proof: &Proof<Bn254>,
    public_inputs: &RevealPublicInputs,
) -> Result<bool, SynthesisError> {
    let ark_public_inputs = public_inputs.to_ark_public_inputs();
    Groth16::<Bn254>::verify(vk, &ark_public_inputs, proof)
}

pub fn verify_shuffle_proof(
    vk: &VerifyingKey<Bn254>,
    proof: &Proof<Bn254>,
    public_inputs: &ShufflePublicInputs,
) -> Result<bool, SynthesisError> {
    let ark_public_inputs = public_inputs.to_ark_public_inputs();
    Groth16::<Bn254>::verify(vk, &ark_public_inputs, proof)
}
