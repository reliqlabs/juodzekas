use cosmwasm_std::{DepsMut, Env, MessageInfo, Response, StdError};
use cw2::set_contract_version;
use crate::error::ContractError;
use crate::msg::InstantiateMsg;
use crate::state::{Config, CONFIG, DEALER, DEALER_BALANCE, GAME_COUNTER};

const CONTRACT_NAME: &str = "crates.io:juodzekas";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    // Validate payout ratios
    if msg.blackjack_payout.denominator == 0
        || msg.standard_payout.denominator == 0
        || msg.insurance_payout.denominator == 0
    {
        return Err(ContractError::Std(StdError::msg("Payout ratio denominator cannot be zero")));
    }

    // Validate bet limits
    if msg.min_bet.is_zero() {
        return Err(ContractError::Std(StdError::msg("min_bet must be greater than zero")));
    }
    if msg.min_bet > msg.max_bet {
        return Err(ContractError::Std(StdError::msg("min_bet cannot exceed max_bet")));
    }

    let config = Config {
        denom: msg.denom.clone(),
        min_bet: msg.min_bet,
        max_bet: msg.max_bet,
        blackjack_payout: msg.blackjack_payout,
        insurance_payout: msg.insurance_payout,
        standard_payout: msg.standard_payout,
        dealer_hits_soft_17: msg.dealer_hits_soft_17,
        dealer_peeks: msg.dealer_peeks,
        double_restriction: msg.double_restriction,
        max_splits: msg.max_splits,
        can_split_aces: msg.can_split_aces,
        can_hit_split_aces: msg.can_hit_split_aces,
        surrender_allowed: msg.surrender_allowed,
        shuffle_vk_id: msg.shuffle_vk_id.clone(),
        reveal_vk_id: msg.reveal_vk_id.clone(),
        timeout_seconds: msg.timeout_seconds.unwrap_or(3600),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    GAME_COUNTER.save(deps.storage, &0u64)?;

    // Single-dealer: the instantiator is the dealer
    DEALER.save(deps.storage, &info.sender)?;

    // Extract initial bankroll from sent funds
    let initial_balance = info.funds.iter()
        .find(|c| c.denom == msg.denom)
        .map(|c| cosmwasm_std::Uint128::try_from(c.amount).unwrap_or(cosmwasm_std::Uint128::MAX))
        .unwrap_or(cosmwasm_std::Uint128::zero());
    DEALER_BALANCE.save(deps.storage, &initial_balance)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("dealer", info.sender)
        .add_attribute("denom", msg.denom)
        .add_attribute("min_bet", msg.min_bet)
        .add_attribute("max_bet", msg.max_bet)
        .add_attribute("initial_balance", initial_balance))
}
