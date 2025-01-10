use std::{
    sync::mpsc::{self, Receiver, SyncSender},
};

use alloy_primitives::{keccak256, U64};
use arbutil::{evm::EvmData, Bytes20, Bytes32};
use precompile::Bytes;
use revm::{
    context::Cfg,
    context_interface::{Block, Transaction},
    handler::FrameResult,
    interpreter::{
        interpreter::ExtBytecode, interpreter_types::LegacyBytecode, CallInputs, CallScheme,
        CallValue, FrameInput, Host, InputsImpl, InterpreterAction,
    },
    primitives::U256,
    state::Bytecode,
};
use stylus::prover::programs::config::StylusConfig;

use super::handler::{stylus_call, EvmApiOutcome, EvmApiRequest, StylusOutcome};

pub struct StylusInterpreter {
    bytecode: ExtBytecode,
    inputs: InputsImpl,
    is_static: bool,
    //is_eof_init: bool,
    // spec_id: SpecId,
    gas_limit: u64,

    channels: Option<(SyncSender<EvmApiOutcome>, Receiver<EvmApiRequest>)>,
}

impl StylusInterpreter {
    pub fn new(
        bytecode: Bytecode,
        inputs: InputsImpl,
        is_static: bool,
        //is_eof_init: bool,
        //spec_id: SpecId,
        gas_limit: u64,
    ) -> Self {
        Self {
            bytecode: ExtBytecode::new(bytecode),
            inputs,
            is_static,
            gas_limit,
            channels: None,
        }
    }

    pub fn run<H: Host>(&mut self, host: &mut H) -> InterpreterAction {
        if self.channels.is_none() {
            let (tothread_tx, tothread_rx) = mpsc::sync_channel::<EvmApiOutcome>(0);
            let (fromthread_tx, fromthread_rx) = mpsc::sync_channel::<EvmApiRequest>(0);
            self.channels = Some((tothread_tx, fromthread_rx));

            let evm_data = self.build_evm_data(host);

            let bytecode = Bytes::from(self.bytecode.bytecode_slice()[4..].to_owned());
            stylus_call(
                bytecode,
                self.inputs.input.clone(),
                StylusConfig::default(),
                evm_data,
                self.gas_limit,
                fromthread_tx,
                tothread_rx,
            )
        }

        let mut handler = |request| match request {
            EvmApiRequest::GetBytes32(slot) => {
                let address = self.inputs.target_address;

                let value = if let Some(value) = host.sload(address, slot) {
                    value.data
                } else {
                    U256::ZERO
                };

                EvmApiOutcome::GetBytes32(value, 0)
            }
            EvmApiRequest::AddPages(_) => EvmApiOutcome::AddPages(0),
            EvmApiRequest::SetTrieSlots(key, value, _, _) => {
                let address = self.inputs.target_address;

                _ = host.sstore(address, key.into(), value.into());
                EvmApiOutcome::SetTrieSlots(0)
            }
            _ => todo!(),
        };

        let (tx, rx) = self.channels.as_ref().unwrap();

        loop {
            let request = rx.recv().unwrap();
            match request {
                EvmApiRequest::GetBytes32(..)
                | EvmApiRequest::SetTrieSlots(..)
                | EvmApiRequest::AccountBalance(..)
                | EvmApiRequest::AccountCode(..)
                | EvmApiRequest::AccountCodeHash(..)
                | EvmApiRequest::AddPages(..)
                | EvmApiRequest::CaptureHostIO(..)
                | EvmApiRequest::EmitLog(..)
                | EvmApiRequest::GetTransientBytes32(..)
                | EvmApiRequest::SetTransientBytes32(..) => {
                    let outcome = handler(request);
                    tx.send(outcome).unwrap();
                }
                EvmApiRequest::ContractCall(call_arguments) => {
                    return InterpreterAction::NewFrame(FrameInput::Call(Box::new(CallInputs {
                        input: call_arguments.calldata,
                        return_memory_offset: 0..0,
                        gas_limit: call_arguments.gas_limit,
                        bytecode_address: call_arguments.address,
                        target_address: call_arguments.address,
                        caller: self.inputs.target_address,
                        value: CallValue::Transfer(call_arguments.value),
                        scheme: CallScheme::Call,
                        is_static: self.is_static,
                        is_eof: false,
                    })));
                }
                EvmApiRequest::DelegateCall(..) | EvmApiRequest::StaticCall(..) => {
                    todo!()
                }
                EvmApiRequest::Create1(..) | EvmApiRequest::Create2(..) => todo!(),
                EvmApiRequest::Return(outcome, _) => {
                    return InterpreterAction::Return {
                        result: outcome.result,
                    }
                }
            };
        }
    }

    fn build_evm_data<H: Host>(&self, host: &mut H) -> EvmData {
        let block = host.block();
        let tx = host.tx();
        let base_fee = block.basefee();

        let evm_data = EvmData {
            arbos_version: 0,
            block_basefee: Bytes32::try_from(block.basefee().to_be_bytes_vec()).unwrap(),
            chainid: host.cfg().chain_id(),
            block_coinbase: Bytes20::try_from(block.beneficiary().as_slice()).unwrap(),
            block_gas_limit: U64::wrapping_from(*block.gas_limit()).to::<u64>(),
            block_number: U64::wrapping_from(*block.number()).to::<u64>(),
            block_timestamp: U64::wrapping_from(*block.timestamp()).to::<u64>(),
            contract_address: Bytes20::try_from(self.inputs.target_address.as_slice()).unwrap(),
            module_hash: Bytes32::try_from(
                keccak256(self.inputs.target_address.as_slice()).as_slice(),
            )
            .unwrap(),
            msg_sender: Bytes20::try_from(self.inputs.caller_address.as_slice()).unwrap(),
            msg_value: Bytes32::try_from(self.inputs.call_value.to_be_bytes_vec()).unwrap(),
            tx_gas_price: Bytes32::from(tx.effective_gas_price(*base_fee).to_be_bytes()),
            tx_origin: Bytes20::try_from(self.inputs.caller_address.as_slice()).unwrap(),
            reentrant: 0,
            return_data_len: 0,
            cached: false,
            tracing: false,
        };

        evm_data
    }

    pub fn return_result(&mut self, result: FrameResult) {
        let (tx, _) = self.channels.as_ref().unwrap();

        let outcome = result.output();

        match outcome {
            revm::context_interface::result::Output::Call(bytes) => {
                if result.instruction_result().is_ok() {
                    _ = tx.send(EvmApiOutcome::Call(
                        StylusOutcome::Return(bytes),
                        result.gas().spent(),
                    ));
                } else {
                    _ = tx.send(EvmApiOutcome::Call(
                        StylusOutcome::Revert(bytes),
                        result.gas().spent(),
                    ));
                }
            }
            revm::context_interface::result::Output::Create(bytes, address) => {
                if result.instruction_result().is_ok() {
                    _ = tx.send(EvmApiOutcome::Create(
                        StylusOutcome::Return(bytes),
                        address.unwrap(),
                        result.gas().spent(),
                    ));
                } else {
                    _ = tx.send(EvmApiOutcome::Create(
                        StylusOutcome::Revert(bytes),
                        address.unwrap(),
                        result.gas().spent(),
                    ));
                }
            }
        };
    }
}
