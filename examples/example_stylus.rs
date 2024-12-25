use alloy_sol_types::sol;
use alloy_sol_types::SolCall;
use ethers_providers::{Http, Provider};
use revm::db::EmptyDBTyped;
use revm::primitives::keccak256;
use revm::primitives::Bytecode;
use revm::{
    db::{CacheDB, EmptyDB, EthersDB},
    primitives::{address, Address, Bytes, ExecutionResult, Output, TxKind, U256},
    Database, Evm,
};

use std::convert::Infallible;
use std::io::Read;
use std::sync::Arc;

// generate abi for the calldata from the human readable interface
sol! {
    function number() external view returns (uint256);
    function setNumber(uint256 new_number) external;
    function setNumberRevert(uint256 new_number) external;
    function forwardTo(address to, bytes calldata data) external returns (bytes memory);

    function forwardStatic(address to, bytes calldata data) external view returns (bytes memory);

    function forwardDelegate(address to, bytes calldata data) external returns (bytes memory);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    
    let target_address = address!("0d4a11d5EEaaC28EC3F61d100daF4d40471f1852");
    let caller = address!("0000000000000000000000000000000000000000");

    // initialise empty in-memory-db
    let mut cache_db = CacheDB::new(EmptyDB::default());

    cache_db.insert_account_info(target_address, revm::primitives::AccountInfo { balance: U256::from(0), nonce: 0, code_hash: keccak256("stylus"), code: Some(Bytecode::new_legacy(Bytes::from(vec![0x00, 0x01]))) });
    
    let number = call_number(&mut cache_db, caller, target_address)?;

    // Print emulated getReserves() call output
    println!("number: {:#?}", number);

    // set new number
    set_number(&mut cache_db, caller, target_address, U256::from(1337))?;

    println!("Number set to 1337");

    let number = call_number(&mut cache_db, caller, target_address)?;

    println!("number: {:#?}", number);

    println!("Forwarding to number contract: {:#?}", forward_to_number(&mut cache_db, caller, target_address)?);

    Ok(())
}

fn call_number(cache_db: &mut CacheDB<EmptyDBTyped<Infallible>>, caller: Address, to: Address) -> anyhow::Result<U256> {

    // initialise an empty (default) EVM
    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            // fill in missing bits of env struct
            // change that to whatever caller you want to be
            tx.caller = caller;
            // account you want to transact with
            tx.transact_to = TxKind::Call(to);
            // calldata formed via abigen
            tx.data = numberCall::new(()).abi_encode().into();
            // transaction value in wei
            tx.value = U256::from(0);
        })
        .build();

    // execute transaction without writing to the DB
    let ref_tx = evm.transact().unwrap();

    // select ExecutionResult struct
    let result = ref_tx.result;

    // unpack output call enum into raw bytes
    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        _ => panic!("Execution failed: {result:?}"),
    };

    // decode bytes to reserves + ts via alloy's abi decode
    let return_vals = numberCall::abi_decode_returns(&value, true)?;

    Ok(return_vals._0)
}

fn set_number(cache_db: &mut CacheDB<EmptyDBTyped<Infallible>>, caller: Address, to: Address, new_number: U256) -> anyhow::Result<()> {
    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            tx.caller = caller;
            tx.transact_to = TxKind::Call(to);
            tx.data = setNumberCall::new((new_number, )).abi_encode().into();
            tx.value = U256::from(0);
        })
        .build();

    let result = evm.transact_commit().unwrap();


    match result {
        ExecutionResult::Success { .. } => Ok(()),
        _ => panic!("Execution failed: {result:?}"),
    }
}

fn forward_to_number(cache_db: &mut CacheDB<EmptyDBTyped<Infallible>>, caller: Address, to: Address) -> anyhow::Result<U256> {
    let calldata = numberCall::new(()).abi_encode();

    let calldata = forwardToCall::new((to, Bytes::from(calldata),)).abi_encode();
    // initialise an empty (default) EVM
    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            // fill in missing bits of env struct
            // change that to whatever caller you want to be
            tx.caller = caller;
            // account you want to transact with
            tx.transact_to = TxKind::Call(to);
            // calldata formed via abigen
            tx.data = calldata.into();
            // transaction value in wei
            tx.value = U256::from(0);
        })
        .build();

    // execute transaction without writing to the DB
    let ref_tx = evm.transact().unwrap();

    // select ExecutionResult struct
    let result = ref_tx.result;

    println!("result: {:#?}", result);

    // unpack output call enum into raw bytes
    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        _ => panic!("Execution failed: {result:?}"),
    };

    // decode bytes to reserves + ts via alloy's abi decode
    let return_vals = numberCall::abi_decode_returns(&value, true)?;

    Ok(return_vals._0)
}

fn forward_to_number_nested(cache_db: &mut CacheDB<EmptyDBTyped<Infallible>>, caller: Address, to: Address) -> anyhow::Result<U256> {
    let calldata = numberCall::new(()).abi_encode();

    let calldata = forwardToCall::new((to, Bytes::from(calldata),)).abi_encode();

    let calldata = forwardToCall::new((to, Bytes::from(calldata),)).abi_encode();
    // initialise an empty (default) EVM
    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            // fill in missing bits of env struct
            // change that to whatever caller you want to be
            tx.caller = caller;
            // account you want to transact with
            tx.transact_to = TxKind::Call(to);
            // calldata formed via abigen
            tx.data = calldata.into();
            // transaction value in wei
            tx.value = U256::from(0);
        })
        .build();

    // execute transaction without writing to the DB
    let ref_tx = evm.transact().unwrap();

    // select ExecutionResult struct
    let result = ref_tx.result;

    println!("result: {:#?}", result);

    // unpack output call enum into raw bytes
    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        _ => panic!("Execution failed: {result:?}"),
    };

    // decode bytes to reserves + ts via alloy's abi decode
    let return_vals = numberCall::abi_decode_returns(&value, true)?;

    Ok(return_vals._0)
}