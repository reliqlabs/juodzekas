use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, Deps, StdResult, StdError};

#[cw_serde]
pub struct QueryVerifyRequest {
    pub proof: Binary,
    pub public_inputs: Vec<String>,
    pub vkey_name: String,
    pub vkey_id: u64,
}

#[cw_serde]
pub struct ProofVerifyResponse {
    pub verified: bool,
}

pub fn xion_zk_verify(
    deps: Deps,
    vkey_name: &str,
    proof: Binary,
    public_inputs: Vec<String>,
) -> StdResult<bool> {
    // In test mode, we might want to bypass or mock this
    #[cfg(test)]
    {
        if proof == Binary::from(b"valid_proof") {
            return Ok(true);
        }
    }

    let query_request = QueryVerifyRequest {
        proof,
        public_inputs,
        vkey_name: vkey_name.to_string(),
        vkey_id: 0,
    };

    // Stargate query path from Xion source
    let path = "/xion.zk.v1.Query/ProofVerify".to_string();
    let data = Binary::from(serde_json_wasm::to_vec(&query_request).map_err(|e| StdError::msg(e.to_string()))?);

    let query = cosmwasm_std::QueryRequest::Stargate {
        path,
        data,
    };

    let res: ProofVerifyResponse = deps.querier.query(&query)?;
    Ok(res.verified)
}
