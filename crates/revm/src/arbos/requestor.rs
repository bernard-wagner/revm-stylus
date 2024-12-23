// Copyright 2023-2024, Offchain Labs, Inc.
// For license information, see https://github.com/OffchainLabs/nitro/blob/master/LICENSE

#![allow(clippy::too_many_arguments)]

use arbutil::evm::api::{Gas, Ink, VecReader};
use arbutil::evm::user::UserOutcomeKind;
use arbutil::evm::{
    api::{EvmApiMethod, EVM_API_METHOD_REQ_OFFSET},
    req::EvmApiRequestor,
    req::RequestHandler,
    user::UserOutcome,
    EvmData,
};
use eyre::{eyre, Result};
use revm_interpreter::instructions::data;
use revm_interpreter::{gas, CallInputs, CallOutcome, CreateInputs, CreateOutcome};
use revm_precompile::B256;
use stylus::env::{Escape, MaybeEscape};
use stylus::prover::programs::config::{CompileConfig, StylusConfig};
use std::os::macos::raw::stat;
use std::thread;
use std::time::Duration;
use std::{
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
};
use stylus::{native::NativeInstance, run::RunProgram};
use crate::primitives::{Address,Bytes, U256};

use crate::arbos::revm_types;


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

    // At the end of execution
    StylusOutcome(StylusOutcome)
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
    GetTrieSlots(u64),
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

    // Simpe ACK
    StylusOutcome,
}


#[derive(Clone)]
pub enum StylusOutcome {
    Return(Bytes),
    Revert(Bytes),
    Failure,
    OutOfInk,
    OutOfStack,
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
            },
            EvmApiMethod::SetTrieSlots => {
                let gas_left = revm_types::take_u64(&mut data);
                let key = revm_types::take_bytes32(&mut data);
                let value = revm_types::take_bytes32(&mut data);
                
                EvmApiRequest::SetTrieSlots(key, value, revm_types::take_rest(&mut data), gas_left)
  
            },
            EvmApiMethod::GetTransientBytes32 => {
                EvmApiRequest::GetTransientBytes32(revm_types::take_rest(&mut data))
            }
            EvmApiMethod::SetTransientBytes32 => {
                EvmApiRequest::SetTransientBytes32(revm_types::take_rest(&mut data))
            }
            EvmApiMethod::ContractCall | EvmApiMethod::DelegateCall | EvmApiMethod::StaticCall  => {
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
            },
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
            },
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
            },
            EvmApiMethod::EmitLog => {
                let topic_count = revm_types::take_u32(&mut data);
                
                let mut topics = vec![];
                
                for _ in 0..topic_count {
                    let hash = revm_types::take_bytes32(&mut data);
                    topics.push(hash);
                };
                
                let data = revm_types::take_rest(&mut data);

                EvmApiRequest::EmitLog(topics, data)

            },
            EvmApiMethod::AccountBalance => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountBalance(address)
            },
            EvmApiMethod::AccountCode => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountCode(address)
            },
            EvmApiMethod::AccountCodeHash => {
                let address = revm_types::take_address(&mut data);
                EvmApiRequest::AccountCodeHash(address)
            },
            EvmApiMethod::AddPages => {
                let count = revm_types::take_u16(&mut data);
                EvmApiRequest::AddPages(count)
            },
            EvmApiMethod::CaptureHostIO => {
                EvmApiRequest::CaptureHostIO(revm_types::take_rest(&mut data))
            },
        };

        if let Err(error) = self.tx.send(msg) {
            panic!("failed sending request from cothread: {error}");
        }
        match self.rx.recv() {
            Ok(response) => {
                match response {
                    EvmApiOutcome::GetBytes32(data, gas_cost) => {
                        (data.to_be_bytes_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::GetTrieSlots(gas_cost)=> {                        
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::GetTransientBytes32(data, gas_cost) => {
                        (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::SetTransientBytes32(gas_cost) => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountBalance(data, gas_cost) => {
                        (data.to_be_bytes_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountCode(data, gas_cost) => {
                        (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountCodeHash(data, gas_cost) => {
                        (Status::Success.into(), VecReader::new(data.to_vec()), Gas(gas_cost))
                    },
                    EvmApiOutcome::CaptureHostIO(gas_cost) => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::Call(stylus_outcome, gas_cost) => {
                        let (result, data) = match stylus_outcome {
                            StylusOutcome::Return(data) => (Status::Success, data),
                            StylusOutcome::Revert(data) => (Status::Failure, data),
                            StylusOutcome::Failure => (Status::Failure, vec![].into()),
                            StylusOutcome::OutOfInk => (Status::OutOfGas, vec![].into()),
                            StylusOutcome::OutOfStack => (Status::WriteProtection, vec![].into()),
                        };

                        (result.into(), VecReader::new(data.to_vec()), Gas(gas_cost))
                    },
                    EvmApiOutcome::Create(stylus_outcome, address, gas_cost) => {
                        let (status, data) = match stylus_outcome {
                            StylusOutcome::Return(data) => (Status::Success, data),
                            StylusOutcome::Revert(data) => (Status::Failure, data),
                            StylusOutcome::Failure => (Status::Failure, vec![].into()),
                            StylusOutcome::OutOfInk => (Status::OutOfGas, vec![].into()),
                            StylusOutcome::OutOfStack => (Status::WriteProtection, vec![].into()),
                        };

                        let result = [status.into(), address.to_vec()].concat();

                        (result.into(), VecReader::new(data.to_vec()), Gas(gas_cost))
                    },
                    EvmApiOutcome::EmitLog(gas_cost) => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AddPages(gas_cost) => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::StylusOutcome => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(0))
                    },
                }
            },
            Err(_) => panic!("no response from main thread"),
        }
    }
}

struct CothreadHandler {
    tx: SyncSender<EvmApiOutcome>,
    rx: Receiver<EvmApiRequest>,
    thread: Option<JoinHandle<MaybeEscape>>,
    last_request: Option<EvmApiRequest>,
}

impl CothreadHandler {
    pub fn wait_next_message(&mut self) -> MaybeEscape {
        let msg = self.rx.recv_timeout(Duration::from_secs(10));
        let Ok(msg) = msg else {
            return MaybeEscape::Err(Escape::Exit(1));
        };
        self.last_request = Some(msg); // TODO: Ids
        Ok(())
    }

    pub fn wait_done(&mut self) -> MaybeEscape {
        let error = || Escape::Exit(2);
        let status = self.thread.take().ok_or_else(error)?.join();
        match status {
            Ok(res) => res,
            Err(_) => MaybeEscape::Err(Escape::Exit(3)),
        }
    }

    pub fn last_message(&self) -> Result<EvmApiRequest, Escape> {
        self.last_request
            .clone()
            .ok_or_else(|| Escape::Exit(4))
    }

    pub fn set_response(
        &mut self,
        result: EvmApiOutcome
    ) -> MaybeEscape {
        // let Some(msg) = self.last_request.clone() else {
        //     return MaybeEscape::Err(Escape::Exit(5));
        // };
        // if msg.1 != id {
        //     return MaybeEscape::Err(Escape::Exit(6));
        // };
        
        if let Err(_) = self.tx.send(result) {
            return MaybeEscape::Err(Escape::Exit(7));
        };
        Ok(())
    }
}

// struct StylusHandler {
//     pub callback: Box<dyn FnMut(EvmApiRequest) -> EvmApiOutcome + Send>,
// }

// impl RequestHandler<VecReader> for StylusHandler {
//     fn request(
//         &mut self,
//         req_type: EvmApiMethod,
//         req_data: impl AsRef<[u8]>,
//     ) -> (Vec<u8>, VecReader, Gas) {
//         let mut data = req_data.as_ref().to_vec();
//         let msg = match req_type {
//             EvmApiMethod::GetBytes32 => {
//                 let data = revm_types::take_u256(&mut data);
//                 EvmApiRequest::GetBytes32(data)
//             },
//             EvmApiMethod::SetTrieSlots => {
//                 let gas_left = revm_types::take_u64(&mut data);
//                 let key = revm_types::take_bytes32(&mut data);
//                 let value = revm_types::take_bytes32(&mut data);
                
//                 EvmApiRequest::SetTrieSlots(key, value, revm_types::take_rest(&mut data), gas_left)
  
//             },
//             EvmApiMethod::GetTransientBytes32 => {
//                 EvmApiRequest::GetTransientBytes32(revm_types::take_rest(&mut data))
//             }
//             EvmApiMethod::SetTransientBytes32 => {
//                 EvmApiRequest::SetTransientBytes32(revm_types::take_rest(&mut data))
//             }
//             EvmApiMethod::ContractCall | EvmApiMethod::DelegateCall | EvmApiMethod::StaticCall  => {
//                 let address = revm_types::take_address(&mut data);
//                 let value = revm_types::take_u256(&mut data);
//                 let _ = revm_types::take_u64(&mut data);
//                 let gas_limit = revm_types::take_u64(&mut data);
//                 let calldata = revm_types::take_rest(&mut data);

//                 let calldata = Bytes::from(data[68..].to_vec());
//                 let call_type = match req_type {
//                     EvmApiMethod::ContractCall => CallType::ContractCall,
//                     EvmApiMethod::DelegateCall => CallType::DelegateCall,
//                     EvmApiMethod::StaticCall => CallType::StaticCall,
//                     _ => unreachable!(),
//                 };
                
//                 EvmApiRequest::ContractCall(CallArguments {
//                     address,
//                     value,
//                     gas_limit,
//                     calldata,
//                     call_type,
//                 })
//             },
//             _ => todo!(),
//         };

//         let result = (self.callback)(msg).unwrap();

//         match result {
//             EvmApiOutcome::GetBytes32(data, gas_cost) => {
//                 (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
//             },
//             _ => todo!(),
//         }

//     }
// }

// pub fn exec_wasm_sync(module: &str,
//     calldata: Vec<u8>,
//     config: StylusConfig,
//     evm_data: EvmData,
//     ink: Ink,
//     callback:  Box<dyn FnMut(EvmApiRequest) -> EvmApiOutcome>,
// ) -> Result<EvmApiOutcome> {

//     let handler = StylusHandler {
//         callback,
//     };

//     let instance = NativeInstance::from_path(
//         module, 
//         EvmApiRequestor::new(handler),
//         evm_data,
//         &CompileConfig::default(),
//         config,
//         wasmer_types::compilation::target::Target::default(),
//     );

//     let mut instance = match instance {
//         Ok(instance) => instance,
//         Err(error) => Err(eyre!("failed to deserialize instance: {error}"))?,
//     };

//     let outcome = instance.run_main(&calldata, config, ink);

//     let outcome = match outcome {
//         Err(e) | Ok(UserOutcome::Failure(e)) => UserOutcome::Failure(e.wrap_err("call failed")),
//         Ok(outcome) => outcome,
//     };

//     let (out_kind, data) = outcome.into_data();

//     let outcome = match out_kind {
//         UserOutcomeKind::Success => StylusOutcome::Return(data.into()),
//         UserOutcomeKind::Revert => StylusOutcome::Revert(data.into()),
//         UserOutcomeKind::Failure => StylusOutcome::Failure,
//         UserOutcomeKind::OutOfInk => StylusOutcome::OutOfInk,
//         UserOutcomeKind::OutOfStack => StylusOutcome::OutOfStack,
//     };

//     Ok(EvmApiOutcome::ContractCall(outcome, 0))


// }


/// Executes a wasm on a new thread
pub fn exec_wasm(
    module: &str,
    calldata: Vec<u8>,
    config: StylusConfig,
    evm_data: EvmData,
    ink: Ink,
    tx: SyncSender<EvmApiRequest>,
    rx: Receiver<EvmApiOutcome>,
) {
    // let (tothread_tx, tothread_rx) = mpsc::sync_channel::<EvmApiOutcome>(0);
    // let (fromthread_tx, fromthread_rx) = mpsc::sync_channel::<EvmApiRequest>(0);

    let cothread = StylusRequestor {
        tx, rx
    };

    let evm_api = EvmApiRequestor::new(cothread);

    let mut instance = NativeInstance::from_path(
        module, 
        evm_api, 
        evm_data,
        &CompileConfig::default(),
        config,
        wasmer_types::compilation::target::Target::default(),

    ).unwrap();


    // TODO handle join 
    thread::spawn(move || {
        let outcome = instance.run_main(&calldata, config, ink);
       
        let outcome = match outcome {
            Err(e) | Ok(UserOutcome::Failure(e)) => UserOutcome::Failure(e.wrap_err("call failed")),
            Ok(outcome) => outcome,
        };

        let (out_kind, data) = outcome.into_data();
        
        let outcome = match out_kind {
            UserOutcomeKind::Success => StylusOutcome::Return(data.into()),
            UserOutcomeKind::Revert => StylusOutcome::Revert(data.into()),
            UserOutcomeKind::Failure => StylusOutcome::Failure,
            UserOutcomeKind::OutOfInk => StylusOutcome::OutOfInk,
            UserOutcomeKind::OutOfStack => StylusOutcome::OutOfStack,
        };
       
        instance
            .env_mut()
            .evm_api
            .request_handler()
            .tx
            .send(EvmApiRequest::StylusOutcome(outcome))
            .or_else(|_| Err(Escape::Exit(1)))
    });
}
