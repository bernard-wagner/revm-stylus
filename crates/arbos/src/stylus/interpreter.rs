use alloy_primitives::{keccak256, Bytes, U256, U64};
use arbutil::{
    evm::{
        api::Gas as ArbOsGas,
        req::EvmApiRequestor,
        user::{UserOutcome, UserOutcomeKind},
        EvmData,
    },
    Bytes20, Bytes32,
};
use revm::{
    context::Cfg,
    context_interface::{Block, BlockGetter, CfgGetter, Transaction},
    interpreter::{Gas as RevmGas, Host, InputsImpl, InterpreterAction, InterpreterResult},
};
use stylus::prover::programs::meter::MeteredMachine;
use stylus::{
    native::NativeInstance,
    prover::programs::config::{CompileConfig, StylusConfig},
    run::RunProgram,
};

use super::handler::{FrameCreateFunc, StylusHandler};

pub struct StylusInterpreter<CTX> {
    bytecode: Bytes,
    inputs: InputsImpl,
    is_static: bool,
    gas_limit: u64,
    _phantom: core::marker::PhantomData<CTX>,
}

impl<CTX> StylusInterpreter<CTX> {
    pub fn new(bytecode: Bytes, inputs: InputsImpl, is_static: bool, gas_limit: u64) -> Self {
        Self {
            bytecode,
            inputs,
            is_static,
            gas_limit,
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<CTX: Host + BlockGetter + CfgGetter + Send + 'static> StylusInterpreter<CTX> {
    fn build_evm_data(&self, host: &mut CTX) -> EvmData {
        let block = host.block();
        let tx = host.tx();
        let base_fee = block.basefee();

        let evm_data = EvmData {
            arbos_version: 0,
            block_basefee: Bytes32::from(U256::from(block.basefee()).to_be_bytes()),
            chainid: host.cfg().chain_id(),
            block_coinbase: Bytes20::try_from(block.beneficiary().as_slice()).unwrap(),
            block_gas_limit: U64::wrapping_from(block.gas_limit()).to::<u64>(),
            block_number: U64::wrapping_from(block.number()).to::<u64>(),
            block_timestamp: U64::wrapping_from(block.timestamp()).to::<u64>(),
            contract_address: Bytes20::try_from(self.inputs.target_address.as_slice()).unwrap(),
            module_hash: Bytes32::try_from(
                keccak256(self.inputs.target_address.as_slice()).as_slice(),
            )
            .unwrap(),
            msg_sender: Bytes20::try_from(self.inputs.caller_address.as_slice()).unwrap(),
            msg_value: Bytes32::try_from(self.inputs.call_value.to_be_bytes_vec()).unwrap(),
            tx_gas_price: Bytes32::from(
                U256::from(tx.effective_gas_price(base_fee as u128)).to_be_bytes(),
            ),
            tx_origin: Bytes20::try_from(self.inputs.caller_address.as_slice()).unwrap(),
            reentrant: 0,
            return_data_len: 0,
            cached: false,
            tracing: false,
        };

        evm_data
    }

    pub fn run(&self, context: &mut CTX, cb: FrameCreateFunc<CTX>) -> InterpreterAction {
        let evm_api = EvmApiRequestor::new(StylusHandler::new(
            context,
            self.inputs.target_address,
            cb,
            self.is_static,
        ));

        let evm_data = self.build_evm_data(context);

        let stylus_config = StylusConfig::default();

        let mut instance = NativeInstance::from_bytecode(
            &self.bytecode,
            evm_api,
            evm_data,
            CompileConfig::default(),
            stylus_config,
            wasmer_types::compilation::target::Target::default(),
        )
        .unwrap();

        let ink_limit = stylus_config.pricing.gas_to_ink(ArbOsGas(self.gas_limit));
        let mut gas = RevmGas::new(self.gas_limit);
        gas.spend_all();

        let outcome = instance.run_main(&self.inputs.input, stylus_config, ink_limit);

        let outcome = match outcome {
            Err(e) | Ok(UserOutcome::Failure(e)) => UserOutcome::Failure(e.wrap_err("call failed")),
            Ok(outcome) => outcome,
        };

        let ink_left = instance.ink_left();

        let mut gas_left = stylus_config.pricing.ink_to_gas(ink_left.into()).0;

        let (kind, data) = outcome.into_data();

        let result = match kind {
            UserOutcomeKind::Success => revm::interpreter::InstructionResult::Return,
            UserOutcomeKind::Revert => revm::interpreter::InstructionResult::Revert,
            UserOutcomeKind::Failure => revm::interpreter::InstructionResult::FatalExternalError,
            UserOutcomeKind::OutOfInk => revm::interpreter::InstructionResult::OutOfGas,
            UserOutcomeKind::OutOfStack => {
                gas_left = 0;
                revm::interpreter::InstructionResult::FatalExternalError
            }
        };

        gas.erase_cost(gas_left);

        let output = data.into();

        InterpreterAction::Return {
            result: InterpreterResult {
                result,
                output,
                gas,
            },
        }
    }
}
