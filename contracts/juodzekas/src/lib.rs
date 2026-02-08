pub mod contract;
pub mod error;
pub mod game_logic;
pub mod msg;
pub mod state;
pub mod zk;

#[cfg(not(feature = "library"))]
use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
#[cfg(not(feature = "library"))]
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
#[cfg(not(feature = "library"))]
use crate::error::ContractError;

#[cfg(not(feature = "library"))]
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    contract::instantiate(deps, env, info, msg)
}

#[cfg(not(feature = "library"))]
#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    contract::execute(deps, env, info, msg)
}

#[cfg(not(feature = "library"))]
#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    contract::query(deps, env, msg)
}

// the random function must be disabled in cosmwasm
use core::num::NonZeroU32;
use getrandom::Error;

pub fn always_fail(_buf: &mut [u8]) -> Result<(), Error> {
    let code = NonZeroU32::new(Error::CUSTOM_START).unwrap();
    Err(Error::from(code))
}
use getrandom::register_custom_getrandom;
register_custom_getrandom!(always_fail);
