//! Optimism-specific constants, types, and helpers.
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

use anyhow::{anyhow, bail};
use database::CacheDB;
use revm::{
    context::Context,
    context_interface::DatabaseGetter,
    database_interface::EmptyDB,
    handler::{EthPostExecution, EthPreExecution, EthValidation},
    primitives::{address, keccak256, Bytes, TxKind, U256},
    state::Bytecode,
};
use revm_arbos::{
    evm::ArbOsEvm,
    frame::STYLUS_MAGIC_BYTES,
    handler::{ArbOsExecution, ArbOsHandler},
};

use alloy_sol_types::sol;
use alloy_sol_types::SolCall;

/// Load storage from slot zero to memory
const RUNTIME_BYTECODE: &[u8] = include_bytes!("../stylus_hello_world.wasm");

// generate abi for the calldata from the human readable interface
sol! {
    function number() external view returns (uint256);
    function setNumber(uint256 new_number) external;
    function setNumberRevert(uint256 new_number) external;
    function forwardTo(address to, bytes calldata data) external returns (bytes memory);

    function forwardStatic(address to, bytes calldata data) external view returns (bytes memory);

    function forwardDelegate(address to, bytes calldata data) external returns (bytes memory);
}

fn main() -> anyhow::Result<()> {
    let address = address!("Bd770416a3345F91E4B34576cb804a576fa48EB1");
    let param = 1337;

    let mut evm = ArbOsEvm::new(
        Context::builder().with_db(CacheDB::<EmptyDB>::default()),
        ArbOsHandler::new(
            EthValidation::new(),
            EthPreExecution::new(),
            ArbOsExecution::new(),
            EthPostExecution::new(),
        ),
    );

    let bytecode =
        Bytes::from([STYLUS_MAGIC_BYTES.clone(), Bytes::from(RUNTIME_BYTECODE)].concat());

    evm.context.db().insert_account_info(
        address,
        revm::state::AccountInfo {
            balance: U256::from(0),
            nonce: 0,
            code_hash: keccak256(bytecode.clone()),
            code: Some(Bytecode::new_legacy(bytecode)),
        },
    );

    evm.context.modify_tx(|tx| {
        tx.transact_to = TxKind::Call(address);
        tx.data = setNumberCall::new((U256::from(param),)).abi_encode().into();
    });

    let result = evm.transact()?;
    let Some(storage0) = result
        .state
        .get(&address)
        .ok_or_else(|| anyhow!("Contract not found"))?
        .storage
        .get::<U256>(&Default::default())
    else {
        bail!("Failed to write storage in the init code: {result:#?}");
    };

    println!("storage U256(0) at {address}:  {storage0:#?}");
    assert_eq!(storage0.present_value(), param.try_into()?, "{result:#?}");
    Ok(())
}
