use crate::babyjubjub::Fq;
use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_circom::{CircomBuilder, CircomConfig, WitnessCalculator};
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::{Groth16, Proof, ProvingKey, VerifyingKey};
pub use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::{CircuitSpecificSetupSNARK, SNARK};
use ark_std::rand::{CryptoRng, Rng};
use memmap2::Mmap;
use std::fs::File;

pub fn load_or_generate_keys<R: Rng + CryptoRng>(
    r1cs_path: &str,
    wasm_path: &str,
    pk_cache_path: &str,
    vk_cache_path: &str,
    input_placeholders: Vec<(String, num_bigint::BigInt)>,
    rng: &mut R,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>), Box<dyn std::error::Error>> {
    // Try to load from cache if both files exist
    if std::path::Path::new(pk_cache_path).exists() && std::path::Path::new(vk_cache_path).exists()
    {
        log::info!("Loading cached keys from {pk_cache_path} and {vk_cache_path}");
        match (|| -> Result<_, Box<dyn std::error::Error>> {
            let pk_file = File::open(pk_cache_path)?;
            let vk_file = File::open(vk_cache_path)?;

            log::info!("Deserializing proving key (30-60s)...");
            let pk = ProvingKey::<Bn254>::deserialize_uncompressed_unchecked(pk_file)?;
            log::info!("Proving key loaded successfully");

            log::info!("Deserializing verifying key (5-10s)...");
            let vk = VerifyingKey::<Bn254>::deserialize_uncompressed_unchecked(vk_file)?;
            log::info!("Verifying key loaded successfully");

            Ok((pk, vk))
        })() {
            Ok((pk, vk)) => {
                log::info!("Successfully loaded cached keys");
                return Ok((pk, vk));
            }
            Err(e) => {
                log::warn!("Failed to load cached keys: {e}. Regenerating...");
                // Fall through to regenerate keys
            }
        }
    }

    log::info!("Generating new keys (this may take several minutes for large circuits)...");
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

    log::info!(
        "Caching keys to {pk_cache_path} and {vk_cache_path} (uncompressed for faster loading)..."
    );
    let mut pk_file = File::create(pk_cache_path)?;
    let mut vk_file = File::create(vk_cache_path)?;
    pk.serialize_uncompressed(&mut pk_file)?;
    vk.serialize_uncompressed(&mut vk_file)?;
    log::info!("Successfully cached keys");

    Ok((pk, vk))
}

/// UNSAFE: Load proving keys using memory-mapped files for 10-20x faster loading
///
/// This function memory-maps the cached key files and deserializes directly from the mapped memory.
/// This is MUCH faster than loading through normal file I/O (1-3s vs 30-60s) but comes with risks:
///
/// # Safety
/// - If the cached files are corrupted or modified, this could crash the application
/// - The files must have been serialized with the exact same version of ark-serialize
/// - This skips validation checks that normal deserialization performs
///
/// # Use case
/// Use this for production systems where:
/// - You trust the cached key files
/// - You need fast startup times
/// - You can regenerate keys if corruption is detected
pub unsafe fn load_keys_unsafe_mmap(
    pk_cache_path: &str,
    vk_cache_path: &str,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>), Box<dyn std::error::Error>> {
    log::info!("Loading keys using unsafe memory-mapped files (FAST mode)");
    log::info!("Loading from {pk_cache_path} and {vk_cache_path}");

    // Check files exist
    if !std::path::Path::new(pk_cache_path).exists() {
        return Err(format!("Proving key cache not found: {pk_cache_path}").into());
    }
    if !std::path::Path::new(vk_cache_path).exists() {
        return Err(format!("Verifying key cache not found: {vk_cache_path}").into());
    }

    let start = std::time::Instant::now();

    // Memory-map the proving key file
    let pk_file = File::open(pk_cache_path)?;
    let pk_mmap = Mmap::map(&pk_file)?;
    log::info!("Memory-mapped proving key file ({} bytes)", pk_mmap.len());

    // Deserialize from mmap (this is the unsafe part - assumes file is valid)
    let pk = ProvingKey::<Bn254>::deserialize_uncompressed_unchecked(&pk_mmap[..])?;
    log::info!(
        "Proving key deserialized in {:.2}s",
        start.elapsed().as_secs_f64()
    );

    let vk_start = std::time::Instant::now();

    // Memory-map the verifying key file
    let vk_file = File::open(vk_cache_path)?;
    let vk_mmap = Mmap::map(&vk_file)?;

    let vk = VerifyingKey::<Bn254>::deserialize_uncompressed_unchecked(&vk_mmap[..])?;
    log::info!(
        "Verifying key deserialized in {:.2}s",
        vk_start.elapsed().as_secs_f64()
    );

    let total_time = start.elapsed();
    log::info!(
        "Total mmap loading time: {:.2}s (vs ~30-60s with normal loading)",
        total_time.as_secs_f64()
    );

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
        for f in &self.pk {
            inputs.push(*f);
        }
        for f in &self.ux0 {
            inputs.push(*f);
        }
        for f in &self.ux1 {
            inputs.push(*f);
        }
        for f in &self.vx0 {
            inputs.push(*f);
        }
        for f in &self.vx1 {
            inputs.push(*f);
        }
        for f in &self.s_u {
            inputs.push(*f);
        }
        for f in &self.s_v {
            inputs.push(*f);
        }
        inputs
    }

    pub fn get_input_mapping(&self) -> Vec<(String, Vec<Bn254Fr>)> {
        vec![
            ("pk".to_string(), self.pk.to_vec()),
            ("UX0".to_string(), self.ux0.clone()),
            ("UX1".to_string(), self.ux1.clone()),
            ("VX0".to_string(), self.vx0.clone()),
            ("VX1".to_string(), self.vx1.clone()),
            ("s_u".to_string(), self.s_u.to_vec()),
            ("s_v".to_string(), self.s_v.to_vec()),
        ]
    }
}

/// Rapidsnark proof structure matching snarkjs output
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RapidsnarkProof {
    pub pi_a: [String; 3],
    pub pi_b: [[String; 2]; 3],
    pub pi_c: [String; 3],
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub curve: Option<String>,
}

/// Convert witness Vec<BigInt> to .wtns binary format for rapidsnark
fn witness_bigints_to_wtns(
    witness: &[num_bigint::BigInt],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // .wtns format:
    // - 4 bytes: "wtns" magic
    // - 4 bytes: version (1)
    // - 4 bytes: number of sections (2)
    // - Section 1: Header (section_id=1, length, field_size=32, prime, witness_length)
    // - Section 2: Witness data (section_id=2, length, witness elements as 32-byte LE)

    let mut buffer = Vec::new();

    // Magic number "wtns"
    buffer.extend_from_slice(b"wtns");

    // Version 1
    buffer.extend_from_slice(&1u32.to_le_bytes());

    // Number of sections (2)
    buffer.extend_from_slice(&2u32.to_le_bytes());

    // Section 1: Header
    // Section ID = 1
    buffer.extend_from_slice(&1u32.to_le_bytes());

    // Section 1 length: 4 (field_size) + 32 (prime) + 4 (witness_length) = 40 bytes
    buffer.extend_from_slice(&40u64.to_le_bytes());

    // Field size (32 bytes for BN254)
    buffer.extend_from_slice(&32u32.to_le_bytes());

    // BN254 prime: 21888242871839275222246405745257275088548364400416034343698204186575808495617
    // .wtns format expects the prime in little-endian
    let bn254_prime_hex = "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001";
    let mut prime_bytes = hex::decode(bn254_prime_hex)?;
    prime_bytes.reverse(); // Convert to little-endian
    buffer.extend_from_slice(&prime_bytes);

    // Witness length
    buffer.extend_from_slice(&(witness.len() as u32).to_le_bytes());

    // Section 2: Witness data
    // Section ID = 2
    buffer.extend_from_slice(&2u32.to_le_bytes());

    // Section 2 length: witness.len() * 32 bytes
    let witness_data_len = (witness.len() * 32) as u64;
    buffer.extend_from_slice(&witness_data_len.to_le_bytes());

    // Witness elements (32 bytes each, little-endian)
    for w in witness {
        let (_, bytes) = w.to_bytes_le();
        let mut padded = [0u8; 32];
        padded[..bytes.len()].copy_from_slice(&bytes);
        buffer.extend_from_slice(&padded);
    }

    Ok(buffer)
}

/// Generates a ZK proof using rapidsnark (WASM witness + fast proving).
/// Returns proof in snarkjs/rapidsnark JSON format.
pub fn generate_shuffle_proof_rapidsnark(
    public_inputs: &ShufflePublicInputs,
    private_inputs: Vec<(String, Vec<Bn254Fr>)>,
) -> Result<RapidsnarkProof, Box<dyn std::error::Error>> {
    log::info!("Generating witness using WASM calculator");

    // Load WASM witness calculator
    // Try multiple possible paths (depending on where code is run from)
    let possible_paths = [
        "circuits/circuit-artifacts/wasm/encrypt.wasm",
        "../../circuits/circuit-artifacts/wasm/encrypt.wasm",
        "../../../circuits/circuit-artifacts/wasm/encrypt.wasm",
    ];

    let wasm_path = possible_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("Could not find encrypt.wasm in any expected location")?;

    let mut store = wasmer::Store::default();
    let mut calculator = WitnessCalculator::new(&mut store, wasm_path)?;

    // Build input map for witness calculator
    let mut inputs: Vec<(String, Vec<num_bigint::BigInt>)> = Vec::new();

    // Convert Bn254Fr to BigInt
    let convert_to_bigint = |f: &Bn254Fr| -> num_bigint::BigInt {
        num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le())
    };

    // Add public inputs
    for (name, vals) in public_inputs.get_input_mapping() {
        let bigint_vals: Vec<num_bigint::BigInt> = vals.iter().map(convert_to_bigint).collect();
        inputs.push((name, bigint_vals));
    }

    // Add private inputs
    for (name, vals) in private_inputs {
        let bigint_vals: Vec<num_bigint::BigInt> = vals.iter().map(convert_to_bigint).collect();
        inputs.push((name, bigint_vals));
    }

    // Generate witness
    let witness_bigints = calculator.calculate_witness(&mut store, inputs, false)?;
    log::info!(
        "Witness generated ({} elements), converting to .wtns format",
        witness_bigints.len()
    );

    // Convert witness BigInts to .wtns binary format
    let wtns_bytes = witness_bigints_to_wtns(&witness_bigints)?;
    log::info!(
        "Witness serialized ({} bytes), using rapidsnark for proof",
        wtns_bytes.len()
    );

    // Use rapidsnark for FAST proof generation
    let possible_zkey_paths = [
        "circuits/circuit-artifacts/zkey/encrypt.zkey",
        "../../circuits/circuit-artifacts/zkey/encrypt.zkey",
        "../../../circuits/circuit-artifacts/zkey/encrypt.zkey",
    ];

    let zkey_path = possible_zkey_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("Could not find encrypt.zkey in any expected location")?;

    log::info!("Using zkey: {zkey_path}");
    let rapidsnark_result =
        rust_rapidsnark::groth16_prover_zkey_file_wrapper(zkey_path, wtns_bytes)
            .map_err(|e| format!("Rapidsnark proof generation failed: {e}"))?;

    log::info!("Rapidsnark proof generated successfully");

    // Parse the proof JSON
    let proof: RapidsnarkProof = serde_json::from_str(&rapidsnark_result.proof)?;
    Ok(proof)
}

#[derive(Clone, Debug)]
pub struct RevealPublicInputs {
    pub y: [Bn254Fr; 4],
    pub pk_p: [Bn254Fr; 2],
    pub out: [Bn254Fr; 2], // The decryption output (partial decryption)
}

impl RevealPublicInputs {
    pub fn from_babyjubjub(y: [Fq; 4], pk_p: [Fq; 2], out: [Fq; 2]) -> Self {
        let convert = |f: &Fq| Bn254Fr::from_le_bytes_mod_order(&f.into_bigint().to_bytes_le());
        Self {
            y: [
                convert(&y[0]),
                convert(&y[1]),
                convert(&y[2]),
                convert(&y[3]),
            ],
            pk_p: [convert(&pk_p[0]), convert(&pk_p[1])],
            out: [convert(&out[0]), convert(&out[1])],
        }
    }

    pub fn to_ark_public_inputs(&self) -> Vec<Bn254Fr> {
        let mut inputs = Vec::new();
        // Circuit outputs come first
        for f in &self.out {
            inputs.push(*f);
        }
        // Then the declared public inputs
        for f in &self.y {
            inputs.push(*f);
        }
        for f in &self.pk_p {
            inputs.push(*f);
        }
        inputs
    }
}

/// Generates a ZK proof for the reveal operation using rapidsnark.
/// Returns proof in snarkjs/rapidsnark JSON format.
pub fn generate_reveal_proof_rapidsnark(
    public_inputs: &RevealPublicInputs,
    sk_p: Bn254Fr,
) -> Result<RapidsnarkProof, Box<dyn std::error::Error>> {
    log::info!("Generating reveal witness using WASM calculator");

    // Load WASM witness calculator
    // Try multiple possible paths (depending on where code is run from)
    let possible_paths = [
        "circuits/circuit-artifacts/wasm/decrypt.wasm",
        "../../circuits/circuit-artifacts/wasm/decrypt.wasm",
        "../../../circuits/circuit-artifacts/wasm/decrypt.wasm",
    ];

    let wasm_path = possible_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("Could not find decrypt.wasm in any expected location")?;

    let mut store = wasmer::Store::default();
    let mut calculator = WitnessCalculator::new(&mut store, wasm_path)?;

    // Build input list for witness calculator
    let mut inputs: Vec<(String, Vec<num_bigint::BigInt>)> = Vec::new();

    // Convert Bn254Fr to BigInt
    let convert_to_bigint = |f: &Bn254Fr| -> num_bigint::BigInt {
        num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &f.into_bigint().to_bytes_le())
    };

    inputs.push((
        "Y".to_string(),
        public_inputs.y.iter().map(convert_to_bigint).collect(),
    ));
    inputs.push((
        "pkP".to_string(),
        public_inputs.pk_p.iter().map(convert_to_bigint).collect(),
    ));
    inputs.push(("skP".to_string(), vec![convert_to_bigint(&sk_p)]));

    // Generate witness
    let witness_bigints = calculator.calculate_witness(&mut store, inputs, false)?;
    log::info!(
        "Reveal witness generated ({} elements), converting to .wtns format",
        witness_bigints.len()
    );

    // Convert witness to .wtns binary format
    let wtns_bytes = witness_bigints_to_wtns(&witness_bigints)?;
    log::info!("Reveal witness serialized, using rapidsnark for proof");

    // Use rapidsnark for FAST proof generation
    let possible_zkey_paths = [
        "circuits/circuit-artifacts/zkey/decrypt.zkey",
        "../../circuits/circuit-artifacts/zkey/decrypt.zkey",
        "../../../circuits/circuit-artifacts/zkey/decrypt.zkey",
    ];

    let zkey_path = possible_zkey_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("Could not find decrypt.zkey in any expected location")?;

    let rapidsnark_result =
        rust_rapidsnark::groth16_prover_zkey_file_wrapper(zkey_path, wtns_bytes)
            .map_err(|e| format!("Rapidsnark reveal proof generation failed: {e}"))?;

    log::info!("Rapidsnark reveal proof generated successfully");

    // Parse the proof JSON
    let proof: RapidsnarkProof = serde_json::from_str(&rapidsnark_result.proof)?;
    Ok(proof)
}

/// Verifies a rapidsnark proof using snarkjs verification key
pub fn verify_shuffle_proof_rapidsnark(
    vkey_path: &str,
    proof: &RapidsnarkProof,
    public_inputs: &ShufflePublicInputs,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Load verification key from JSON
    let vkey_json = std::fs::read_to_string(vkey_path)?;

    // Convert public inputs to ark format
    let pub_inputs_ark = public_inputs.to_ark_public_inputs();

    // Parse and verify using arkworks as temporary solution
    // This maintains compatibility while we transition fully to rapidsnark
    let ark_proof = parse_rapidsnark_proof(proof)?;
    let vkey_data: serde_json::Value = serde_json::from_str(&vkey_json)?;
    let ark_vk = parse_snarkjs_vkey(&vkey_data)?;

    Ok(Groth16::<Bn254>::verify(&ark_vk, &pub_inputs_ark, &ark_proof).unwrap_or(false))
}

/// Verifies a rapidsnark reveal proof using snarkjs verification key
pub fn verify_reveal_proof_rapidsnark(
    vkey_path: &str,
    proof: &RapidsnarkProof,
    public_inputs: &RevealPublicInputs,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Load verification key from JSON
    let vkey_json = std::fs::read_to_string(vkey_path)?;

    // Convert public inputs to ark format
    let pub_inputs_ark = public_inputs.to_ark_public_inputs();

    // Parse and verify using arkworks as temporary solution
    let ark_proof = parse_rapidsnark_proof(proof)?;
    let vkey_data: serde_json::Value = serde_json::from_str(&vkey_json)?;
    let ark_vk = parse_snarkjs_vkey(&vkey_data)?;

    Ok(Groth16::<Bn254>::verify(&ark_vk, &pub_inputs_ark, &ark_proof).unwrap_or(false))
}

/// Parse rapidsnark/snarkjs proof JSON to arkworks format
fn parse_rapidsnark_proof(
    proof: &RapidsnarkProof,
) -> Result<Proof<Bn254>, Box<dyn std::error::Error>> {
    use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};

    // Helper to parse decimal string to field element
    let parse_fq = |s: &str| -> Result<Fq, Box<dyn std::error::Error>> {
        let bigint = num_bigint::BigInt::parse_bytes(s.as_bytes(), 10)
            .ok_or("Failed to parse decimal string")?;
        let bytes = bigint.to_bytes_le().1; // Get little-endian bytes
        Ok(Fq::from_le_bytes_mod_order(&bytes))
    };

    // Parse pi_a (G1 point)
    let a_x = parse_fq(&proof.pi_a[0])?;
    let a_y = parse_fq(&proof.pi_a[1])?;
    let a = G1Affine::new_unchecked(a_x, a_y);

    // Parse pi_b (G2 point)
    let b_x0 = parse_fq(&proof.pi_b[0][0])?;
    let b_x1 = parse_fq(&proof.pi_b[0][1])?;
    let b_y0 = parse_fq(&proof.pi_b[1][0])?;
    let b_y1 = parse_fq(&proof.pi_b[1][1])?;
    let b_x = Fq2::new(b_x0, b_x1);
    let b_y = Fq2::new(b_y0, b_y1);
    let b = G2Affine::new_unchecked(b_x, b_y);

    // Parse pi_c (G1 point)
    let c_x = parse_fq(&proof.pi_c[0])?;
    let c_y = parse_fq(&proof.pi_c[1])?;
    let c = G1Affine::new_unchecked(c_x, c_y);

    Ok(Proof { a, b, c })
}

/// Parse snarkjs verification key JSON to arkworks format
fn parse_snarkjs_vkey(
    vkey: &serde_json::Value,
) -> Result<VerifyingKey<Bn254>, Box<dyn std::error::Error>> {
    use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};

    // Helper to parse decimal string to field element
    let parse_fq = |s: &str| -> Result<Fq, Box<dyn std::error::Error>> {
        let bigint = num_bigint::BigInt::parse_bytes(s.as_bytes(), 10)
            .ok_or("Failed to parse decimal string")?;
        let bytes = bigint.to_bytes_le().1; // Get little-endian bytes
        Ok(Fq::from_le_bytes_mod_order(&bytes))
    };

    // Helper to parse G1 point from [x, y, z] array
    let parse_g1 = |arr: &serde_json::Value| -> Result<G1Affine, Box<dyn std::error::Error>> {
        let x_str = arr[0].as_str().ok_or("Missing x coordinate")?;
        let y_str = arr[1].as_str().ok_or("Missing y coordinate")?;

        let x = parse_fq(x_str)?;
        let y = parse_fq(y_str)?;

        Ok(G1Affine::new_unchecked(x, y))
    };

    // Helper to parse G2 point from [[x0, x1], [y0, y1], [z0, z1]] array
    let parse_g2 = |arr: &serde_json::Value| -> Result<G2Affine, Box<dyn std::error::Error>> {
        let x0_str = arr[0][0].as_str().ok_or("Missing x0 coordinate")?;
        let x1_str = arr[0][1].as_str().ok_or("Missing x1 coordinate")?;
        let y0_str = arr[1][0].as_str().ok_or("Missing y0 coordinate")?;
        let y1_str = arr[1][1].as_str().ok_or("Missing y1 coordinate")?;

        let x0 = parse_fq(x0_str)?;
        let x1 = parse_fq(x1_str)?;
        let y0 = parse_fq(y0_str)?;
        let y1 = parse_fq(y1_str)?;

        let x = Fq2::new(x0, x1);
        let y = Fq2::new(y0, y1);

        Ok(G2Affine::new_unchecked(x, y))
    };

    // Parse all required fields
    let alpha_g1 = parse_g1(&vkey["vk_alpha_1"])?;
    let beta_g2 = parse_g2(&vkey["vk_beta_2"])?;
    let gamma_g2 = parse_g2(&vkey["vk_gamma_2"])?;
    let delta_g2 = parse_g2(&vkey["vk_delta_2"])?;

    // Parse IC (gamma_abc_g1)
    let ic_arr = vkey["IC"].as_array().ok_or("IC field not an array")?;
    let mut gamma_abc_g1 = Vec::with_capacity(ic_arr.len());
    for ic_point in ic_arr {
        gamma_abc_g1.push(parse_g1(ic_point)?);
    }

    Ok(VerifyingKey {
        alpha_g1,
        beta_g2,
        gamma_g2,
        delta_g2,
        gamma_abc_g1,
    })
}
