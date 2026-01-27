use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};
use cw2::set_contract_version;
use crate::error::ContractError;
use crate::msg::InstantiateMsg;
use crate::state::{Config, CONFIG};

const CONTRACT_NAME: &str = "crates.io:juodzekas";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        min_bet: msg.min_bet,
        max_bet: msg.max_bet,
        bj_payout_permille: msg.bj_payout_permille,
        insurance_payout_permille: msg.insurance_payout_permille,
        standard_payout_permille: msg.standard_payout_permille,
        dealer_hits_soft_17: msg.dealer_hits_soft_17,
        dealer_peeks: msg.dealer_peeks,
        double_down_restriction: msg.double_down_restriction,
        max_splits: msg.max_splits,
        can_split_aces: msg.can_split_aces,
        can_hit_split_aces: msg.can_hit_split_aces,
        surrender_allowed: msg.surrender_allowed,
        shuffle_vk_id: msg.shuffle_vk_id.clone(),
        reveal_vk_id: msg.reveal_vk_id.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("min_bet", msg.min_bet)
        .add_attribute("max_bet", msg.max_bet))
}
