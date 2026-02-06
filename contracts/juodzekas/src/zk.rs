use cosmwasm_std::{Binary, Deps, StdResult};
use prost::Message;
use xion_types::xion::zk::v1::{QueryVerifyRequest, ProofVerifyResponse};
use xion_types::traits::MessageExt;

pub fn xion_zk_verify(
    deps: Deps,
    vkey_name: &str,
    proof: Binary,
    public_inputs: Vec<String>,
) -> StdResult<bool> {
    let query_request = QueryVerifyRequest {
        proof: proof.to_vec(),
        public_inputs,
        vkey_name: vkey_name.to_string(),
        vkey_id: 0,
    };

    let query_bz = query_request.to_bytes()?;
    let query_response = deps.querier.query_grpc(
        String::from("/xion.zk.v1.Query/ProofVerify"),
        Binary::new(query_request.to_bytes()?)
    )?;
    let verify_response = ProofVerifyResponse::decode(query_response.as_slice())?;

    Ok(verify_response.verified)
}
