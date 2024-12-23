use alloy_provider::{network::Ethereum, ProviderBuilder, RootProvider};
use alloy_sol_types::{sol, SolCall, SolValue};
use anyhow::{anyhow, Result};
use alloy_eips::BlockId;
use reqwest::Client;
use revm::{
    db::{CacheDB, EmptyDB},
    primitives::{
        address, keccak256, AccountInfo, Address, Bytes, ExecutionResult, Output, TxKind, U256,
    },
    Evm,
};
use std::ops::Div;
use std::sync::Arc;

type AlloyCacheDB = CacheDB<AlloyDB<Http<Client>, Ethereum, Arc<RootProvider<Http<Client>>>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let client = ProviderBuilder::new().on_http(
        "https://eth-mainnet.g.alchemy.com/v2/YRFEYwmPJQXMP8D4J-HB-ZV2pFGJk33p"
            .parse()
            .unwrap(),
    );
    let client = Arc::new(client);
    let mut cache_db = CacheDB::new(EmptyDB::default());

    // Random empty account
    let account = address!("18B06aaF27d44B756FCF16Ca20C1f183EB49111f");

    // give our test account some fake WETH and ETH
    let one_ether = U256::from(1_000_000_000_000_000_000u128);

    let acc_info = AccountInfo {
        nonce: 0_u64,
        balance: one_ether,
        code_hash: keccak256(Bytes::new()),
        code: None,
    };
    cache_db.insert_account_info(account, acc_info);

    balance_of(account, account, &mut cache_db)?;

    Ok(())
}

fn balance_of(token: Address, address: Address, cache_db: &mut AlloyCacheDB) -> Result<U256> {
    sol! {
        function balanceOf(address account) public returns (uint256);
    }

    let encoded = balanceOfCall { account: address }.abi_encode();

    let mut evm = Evm::builder()
        .with_db(cache_db)
        .modify_tx_env(|tx| {
            // 0x1 because calling USDC proxy from zero address fails
            tx.caller = address!("0000000000000000000000000000000000000001");
            tx.transact_to = TxKind::Call(token);
            tx.data = encoded.into();
            tx.value = U256::from(0);
        })
        .build();

    let ref_tx = evm.transact().unwrap();
    let result = ref_tx.result;

    let value = match result {
        ExecutionResult::Success {
            output: Output::Call(value),
            ..
        } => value,
        result => return Err(anyhow!("'balanceOf' execution failed: {result:?}")),
    };

    let balance = <U256>::abi_decode(&value, false)?;

    Ok(balance)
}
