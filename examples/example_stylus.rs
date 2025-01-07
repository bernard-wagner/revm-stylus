use alloy_sol_types::sol;
use alloy_sol_types::SolCall;
use ethers_providers::{Http, Provider};
use revm::db::EmptyDBTyped;
use revm::primitives::hex;
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

const COUNTER_SOLIDITY: &str = "0x608060405234801561000f575f80fd5b5060043610610060575f3560e01c80631042c9f31461006457806326929eb61461008d5780633fb5c1cb146100a057806380bbeb57146100b45780638381f58a146100c7578063d09de08a146100dd575b5f80fd5b610077610072366004610252565b6100e5565b60405161008491906102dd565b60405180910390f35b61007761009b366004610252565b610190565b6100b26100ae366004610312565b5f55565b005b6100776100c2366004610252565b6101e6565b6100cf5f5481565b604051908152602001610084565b6100b261023d565b60605f80856001600160a01b03168585604051610103929190610329565b5f60405180830381855afa9150503d805f811461013b576040519150601f19603f3d011682016040523d82523d5f602084013e610140565b606091505b5091509150816101875760405162461bcd60e51b815260206004820152600e60248201526d119bdc9dd85c990819985a5b195960921b604482015260640160405180910390fd5b95945050505050565b60605f80856001600160a01b031685856040516101ae929190610329565b5f60405180830381855af49150503d805f811461013b576040519150601f19603f3d011682016040523d82523d5f602084013e610140565b60605f80856001600160a01b03168585604051610204929190610329565b5f604051808303815f865af19150503d805f811461013b576040519150601f19603f3d011682016040523d82523d5f602084013e610140565b5f8054908061024b83610338565b9190505550565b5f805f60408486031215610264575f80fd5b83356001600160a01b038116811461027a575f80fd5b9250602084013567ffffffffffffffff811115610295575f80fd5b8401601f810186136102a5575f80fd5b803567ffffffffffffffff8111156102bb575f80fd5b8660208284010111156102cc575f80fd5b939660209190910195509293505050565b602081525f82518060208401528060208501604085015e5f604082850101526040601f19601f83011684010191505092915050565b5f60208284031215610322575f80fd5b5035919050565b818382375f9101908152919050565b5f6001820161035557634e487b7160e01b5f52601160045260245ffd5b506001019056fea2646970667358221220022dc685d2ea22230c54c8fb51efaa848bbbeef52ce2b9642e050b056ff7d2b364736f6c634300081a0033";

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
    
    let stylus_address = address!("0d4a11d5EEaaC28EC3F61d100daF4d40471f1852");
    let solidity_address = address!("c917e98213a05d271adc5d93d2fee6c1f1006f75");
    let caller = address!("0000000000000000000000000000000000000000");

    // initialise empty in-memory-db
    let mut cache_db = CacheDB::new(EmptyDB::default());

    let counter_bytecode = Bytecode::new_legacy(Bytes::from(hex::decode(COUNTER_SOLIDITY)?));
    cache_db.insert_account_info(solidity_address,  revm::primitives::AccountInfo{
        balance: U256::from(0),
        nonce: 0,
        code_hash: keccak256(counter_bytecode.bytes()),
        code: Some(counter_bytecode),
    });


    cache_db.insert_account_info(stylus_address, revm::primitives::AccountInfo { balance: U256::from(0), nonce: 0, code_hash: keccak256("stylus"), code: Some(Bytecode::new_legacy(Bytes::from(vec![0x00, 0x01]))) });
    
    let number = call_number(&mut cache_db, caller, stylus_address)?;

    // Print emulated getReserves() call output
    println!("number: {:#?}", number);

    // set new number
    set_number(&mut cache_db, caller, stylus_address, U256::from(1337))?;
    set_number(&mut cache_db, caller, solidity_address, U256::from(8080))?;

    println!("-> stylus -> solidity");
    forward_to(&mut cache_db, caller, stylus_address, solidity_address)?;

    println!("-> solidity -> stylus");
    forward_to(&mut cache_db, caller, solidity_address, stylus_address)?;

    println!("-> stylus -> stylus");
    forward_to(&mut cache_db, caller, stylus_address, stylus_address)?;

    println!("-> solidity -> solidity");
    forward_to(&mut cache_db, caller, solidity_address, solidity_address)?;

    //let res = call_number(&mut cache_db, caller, solidity_address)?;

    //println!("Number from solidity: {:#?}", res);

    // let number = call_number(&mut cache_db, caller, stylus_address)?;

    // println!("number: {:#?}", number);

    // println!("Forwarding to number contract: {:#?}", forward_to_number(&mut cache_db, caller, stylus_address)?);

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

    println!("result: {:#?}", value);
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

fn forward_to(cache_db: &mut CacheDB<EmptyDBTyped<Infallible>>, caller: Address, stylus: Address, solidity: Address) -> anyhow::Result<()> {
    let calldata = numberCall::new(()).abi_encode();

    let calldata = forwardToCall::new((solidity, Bytes::from(calldata),)).abi_encode();
    
    //let calldata = forwardToCall::new((stylus, Bytes::from(calldata),)).abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            // fill in missing bits of env struct
            // change that to whatever caller you want to be
            tx.caller = caller;
            // account you want to transact with
            tx.transact_to = TxKind::Call(stylus);
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

    Ok(())
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