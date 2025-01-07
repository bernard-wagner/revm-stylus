// Copyright 2023-2024, Offchain Labs, Inc.
// For license information, see https://github.com/OffchainLabs/nitro/blob/master/LICENSE

#![allow(clippy::too_many_arguments)]

use crate::primitives::{Address, Bytes, U256};
use arbutil::evm::api::{Gas, Ink, VecReader};
use arbutil::evm::user::UserOutcomeKind;
use arbutil::evm::{
    api::EvmApiMethod, req::EvmApiRequestor, req::RequestHandler, user::UserOutcome, EvmData,
};
use ethers_core::types::Call;
use eyre::Result;

use revm_interpreter::{CallOutcome, InterpreterResult};
use revm_precompile::B256;

use std::thread;

use std::{
    sync::mpsc::{Receiver, SyncSender},
    thread::JoinHandle,
};
use stylus::env::Escape;
use stylus::prover::programs::config::{CompileConfig, StylusConfig};
use stylus::{native::NativeInstance, run::RunProgram};

use crate::arbos::revm_types;
use stylus::prover::programs::meter::MeteredMachine;

struct StylusRequestor {
    tx: SyncSender<EvmApiRequest>,
    rx: Receiver<EvmApiOutcome>,
}

#[derive(Clone)]
pub enum EvmApiRequest {
    GetBytes32(U256),
    SetTrieSlots(B256, B256, Bytes, u64),
    GetTransientBytes32(Bytes),
    SetTransientBytes32(Bytes),
    ContractCall(CallArguments),
    DelegateCall(CallArguments),
    StaticCall(CallArguments),
    Create1(CreateArguments),
    Create2(CreateArguments),
    EmitLog(Vec<B256>, Bytes),
    AccountBalance(Address),
    AccountCode(Address),
    AccountCodeHash(Address),
    AddPages(u16),
    CaptureHostIO(Bytes),

    Return(CallOutcome, u64),
}

#[derive(Clone)]
pub struct CallArguments {
    pub address: Address,
    pub value: U256,
    pub gas_limit: u64,
    pub calldata: Bytes,
    pub call_type: CallType,
}

#[derive(Clone)]
pub enum CallType {
    ContractCall,
    DelegateCall,
    StaticCall,
}

#[derive(Clone)]
pub struct CreateArguments {
    pub value: U256,
    pub gas_limit: u64,
    pub salt: B256,
    pub code: Bytes,
    pub create_type: CreateType,
}

#[derive(Clone)]
pub enum CreateType {
    Create1,
    Create2,
}

#[derive(Clone)]
pub enum EvmApiOutcome {
    GetBytes32(U256, u64),
    SetTrieSlots(u64),
    GetTransientBytes32(B256, u64),
    SetTransientBytes32(u64),
    Call(StylusOutcome, u64),
    Create(StylusOutcome, Address, u64),
    EmitLog(u64),
    AccountBalance(U256, u64),
    AccountCode(Bytes, u64),
    AccountCodeHash(B256, u64),
    AddPages(u64),
    CaptureHostIO(u64),
}

#[derive(Clone)]
pub enum StylusOutcome {
    Return(Bytes),
    Revert(Bytes),
    Failure,
    OutOfInk,
}

enum Status {
    Success,
    Failure,
    OutOfGas,
    WriteProtection,
}

impl Into<Vec<u8>> for Status {
    fn into(self) -> Vec<u8> {
        match self {
            Status::Success => vec![0],
            Status::Failure => vec![1],
            Status::OutOfGas => vec![2],
            Status::WriteProtection => vec![3],
        }
    }
}

impl RequestHandler<VecReader> for StylusRequestor {
    fn request(
        &mut self,
        req_type: EvmApiMethod,
        req_data: impl AsRef<[u8]>,
    ) -> (Vec<u8>, VecReader, Gas) {
        let mut data = req_data.as_ref().to_vec();
        let msg = match req_type {
            EvmApiMethod::GetBytes32 => {
                let data = revm_types::take_u256(&mut data);
                EvmApiRequest::GetBytes32(data)
            }
            EvmApiMethod::SetTrieSlots => {
                let gas_left = revm_types::take_u64(&mut data);
                let key = revm_types::take_bytes32(&mut data);
                let value = revm_types::take_bytes32(&mut data);

                EvmApiRequest::SetTrieSlots(key, value, revm_types::take_rest(&mut data), gas_left)
            }
            EvmApiMethod::GetTransientBytes32 => {
                EvmApiRequest::GetTransientBytes32(revm_types::take_rest(&mut data))
            }
            EvmApiMethod::SetTransientBytes32 => {
                EvmApiRequest::SetTransientBytes32(revm_types::take_rest(&mut data))
            }
            EvmApiMethod::ContractCall | EvmApiMethod::DelegateCall | EvmApiMethod::StaticCall => {
                let address = revm_types::take_address(&mut data);
                let value = revm_types::take_u256(&mut data);
                let _ = revm_types::take_u64(&mut data);
                let gas_limit = revm_types::take_u64(&mut data);
                let calldata = revm_types::take_rest(&mut data);

                let call_type = match req_type {
                    EvmApiMethod::ContractCall => CallType::ContractCall,
                    EvmApiMethod::DelegateCall => CallType::DelegateCall,
                    EvmApiMethod::StaticCall => CallType::StaticCall,
                    _ => unreachable!(),
                };

                EvmApiRequest::ContractCall(CallArguments {
                    address,
                    value,
                    gas_limit,
                    calldata,
                    call_type,
                })
            }
            EvmApiMethod::Create1 => {
                let gas_limit = revm_types::take_u64(&mut data);
                let value = revm_types::take_u256(&mut data);
                let code = revm_types::take_rest(&mut data);

                EvmApiRequest::Create1(CreateArguments {
                    value,
                    gas_limit,
                    salt: B256::ZERO,
                    code,
                    create_type: CreateType::Create1,
                })
            }
            EvmApiMethod::Create2 => {
                let gas_limit = revm_types::take_u64(&mut data);
                let value = revm_types::take_u256(&mut data);
                let salt = revm_types::take_bytes32(&mut data);
                let code = revm_types::take_rest(&mut data);

                EvmApiRequest::Create1(CreateArguments {
                    value,
                    gas_limit,
                    salt: salt.into(),
                    code,
                    create_type: CreateType::Create2,
                })
            }
            EvmApiMethod::EmitLog => {
                let topic_count = revm_types::take_u32(&mut data);

                let mut topics = vec![];

                for _ in 0..topic_count {
                    let hash = revm_types::take_bytes32(&mut data);
                    topics.push(hash);
                }

                let data = revm_types::take_rest(&mut data);

                EvmApiRequest::EmitLog(topics, data)
            }
            EvmApiMethod::AccountBalance => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountBalance(address)
            }
            EvmApiMethod::AccountCode => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountCode(address)
            }
            EvmApiMethod::AccountCodeHash => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountCodeHash(address)
            }
            EvmApiMethod::AddPages => {
                let count = revm_types::take_u16(&mut data);
                EvmApiRequest::AddPages(count)
            }
            EvmApiMethod::CaptureHostIO => {
                EvmApiRequest::CaptureHostIO(revm_types::take_rest(&mut data))
            },
        };

        if let Err(error) = self.tx.send(msg) {
            panic!("failed sending request from cothread: {error}");
        }
        match self.rx.recv() {
            Ok(response) => match response {
                EvmApiOutcome::GetBytes32(data, gas_cost) => (
                    data.to_be_bytes_vec(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::SetTrieSlots(gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::GetTransientBytes32(data, gas_cost) => {
                    (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                }
                EvmApiOutcome::SetTransientBytes32(gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::AccountBalance(data, gas_cost) => (
                    data.to_be_bytes_vec(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::AccountCode(data, gas_cost) => {
                    (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                }
                EvmApiOutcome::AccountCodeHash(data, gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(data.to_vec()),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::CaptureHostIO(gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::Call(stylus_outcome, gas_cost) => {
                    
                    let (result, data) = match stylus_outcome {
                        StylusOutcome::Return(data) => (Status::Success, data),
                        StylusOutcome::Revert(data) => (Status::Failure, data),
                        StylusOutcome::Failure => (Status::Failure, vec![].into()),
                        StylusOutcome::OutOfInk => (Status::OutOfGas, vec![].into()),
                    };
                    println!("Call outcome: {:?}", data);
                    (result.into(), VecReader::new(data.to_vec()), Gas(gas_cost))
                }
                EvmApiOutcome::Create(stylus_outcome, address, gas_cost) => {
                    let (status, data) = match stylus_outcome {
                        StylusOutcome::Return(data) => (Status::Success, data),
                        StylusOutcome::Revert(data) => (Status::Failure, data),
                        StylusOutcome::Failure => (Status::Failure, vec![].into()),
                        StylusOutcome::OutOfInk => (Status::OutOfGas, vec![].into()),
                    };

                    let result = [status.into(), address.to_vec()].concat();

                    (result.into(), VecReader::new(data.to_vec()), Gas(gas_cost))
                }
                EvmApiOutcome::EmitLog(gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
                EvmApiOutcome::AddPages(gas_cost) => (
                    Status::Success.into(),
                    VecReader::new(vec![]),
                    Gas(gas_cost),
                ),
            },
            Err(_) => panic!("no response from main thread"),
        }
    }
}

/// Executes a wasm on a new thread
pub fn exec_wasm(
    module: &str,
    calldata: Vec<u8>,
    config: StylusConfig,
    evm_data: EvmData,
    gas: revm_interpreter::Gas,
    tx: SyncSender<EvmApiRequest>,
    rx: Receiver<EvmApiOutcome>,
)  {
    let evm_api = EvmApiRequestor::new(StylusRequestor { tx, rx });

    let mut instance = NativeInstance::from_path(
        module,
        evm_api,
        evm_data,
        &CompileConfig::default(),
        config,
        wasmer_types::compilation::target::Target::default(),
    )
    .unwrap();

    let ink_limit = config.pricing.gas_to_ink(Gas(gas.limit()));

    let mut gas = gas.clone();

    // TODO handle join
    let join = thread::spawn(move || {
        let outcome = instance.run_main(&calldata, config, ink_limit);

        let ink_left = match outcome.as_ref() {
            Ok(UserOutcome::OutOfStack) => Ink(0), // take all ink when out of stack
            _ => instance.ink_left().into(),
        };

        let outcome = match outcome {
            Err(e) | Ok(UserOutcome::Failure(e)) => UserOutcome::Failure(e.wrap_err("call failed")),
            Ok(outcome) => outcome,
        };

        let (out_kind, data) = outcome.into_data();
        let gas_left = config.pricing.ink_to_gas(ink_left);

        if !gas.record_cost(gas.limit()) {
            panic!("gas limit exceeded");
        }

        gas.erase_cost(gas_left.0);

        let outcome = match out_kind {
            UserOutcomeKind::Success => revm_interpreter::InstructionResult::Return,
            UserOutcomeKind::Revert => revm_interpreter::InstructionResult::Revert,
            UserOutcomeKind::Failure => revm_interpreter::InstructionResult::FatalExternalError,
            UserOutcomeKind::OutOfInk => revm_interpreter::InstructionResult::OutOfGas,
            UserOutcomeKind::OutOfStack => revm_interpreter::InstructionResult::CallTooDeep,
        };

        let outcome = CallOutcome {
            memory_offset: 0..0,
            result: InterpreterResult{
                result: outcome,
                output: data.into(),
                gas: gas,
            },

        };
 
        instance.env_mut().evm_api.request_handler().tx.send(EvmApiRequest::Return(outcome, 0)).unwrap();
    });

}
