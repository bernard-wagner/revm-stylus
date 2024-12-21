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
use revm_interpreter::{CallInputs, CallOutcome, CreateInputs};
use stylus::env::{Escape, MaybeEscape};
use stylus::prover::programs::config::{CompileConfig, StylusConfig};
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
use crate::primitives::{Address,Bytes, Log, U256};


struct StylusRequestor {
    tx: SyncSender<EvmApiRequest>,
    rx: Receiver<EvmApiOutcome>,
}


#[derive(Clone)]
pub enum EvmApiRequest {
    GetBytes32(U256),
    SetTrieSlots(U256, U256, Bytes, u64),
    GetTransientBytes32(Bytes),
    SetTransientBytes32(Bytes),
    ContractCall(CallInputs),
    DelegateCall(CallInputs),
    StaticCall(CallInputs),
    Create1(CreateInputs),
    Create2(CreateInputs),
    EmitLog(Log),
    AccountBalance(Address),
    AccountCode(Address),
    AccountCodeHash(Address),
    AddPages(u16),
    CaptureHostIO,

    // At the end of execution
    StylusOutcome(StylusOutcome)
}

#[derive(Clone)]
pub enum EvmApiOutcome {
    GetBytes32(U256, u64),
    SetTrieSlots(u64),
    GetTransientBytes32(U256, u64),
    SetTransientBytes32(u64),
    ContractCall(CallOutcome, Bytes, u64),
    DelegateCall(CallInputs),
    StaticCall(CallInputs),
    Create1(CreateInputs),
    Create2(CreateInputs),
    EmitLog(Log),
    AccountBalance(Address),
    AccountCode(Address),
    AccountCodeHash(Address),
    AddPages(u16),
    CaptureHostIO,

    GetBytes32(Bytes, u64),
    TrieSlots(u64),
    TransientBytes32(Bytes, u64),
    AccountBalance(U256, u64),
    AccountCode(Bytes, u64),
    AccountCodeHash(U256, u64),
    HostIO(Bytes, u64),
}


#[derive(Clone)]
enum StylusOutcome {
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
        let data = req_data.as_ref().to_vec();

        let msg = match req_type {
            EvmApiMethod::GetBytes32 => {
                let data = U256::from_be_slice(data.as_slice());
                EvmApiRequest::GetBytes32(data)
            },
            EvmApiMethod::SetTrieSlots => {
                let gas_left = u64::from_be_bytes(data[0..8].try_into().unwrap());
                let key = U256::from_be_slice(data[8..40].try_into().unwrap());
                let value = U256::from_be_slice(data[40..72].try_into().unwrap());
                let data = Bytes::from(data[72..].to_vec());
                
                EvmApiRequest::SetTrieSlots(key, value, data, gas_left)
  
            },
            EvmApiMethod::GetTransientBytes32 => todo!(),
            EvmApiMethod::SetTransientBytes32 => todo!(),
            EvmApiMethod::ContractCall => todo!(),
            EvmApiMethod::DelegateCall => todo!(),
            EvmApiMethod::StaticCall => todo!(),
            EvmApiMethod::Create1 => todo!(),
            EvmApiMethod::Create2 => todo!(),
            EvmApiMethod::EmitLog => todo!(),
            EvmApiMethod::AccountBalance => todo!(),
            EvmApiMethod::AccountCode => todo!(),
            EvmApiMethod::AccountCodeHash => todo!(),
            EvmApiMethod::AddPages => todo!(),
            EvmApiMethod::CaptureHostIO => todo!(),
        };

        if let Err(error) = self.tx.send(msg) {
            panic!("failed sending request from cothread: {error}");
        }
        match self.rx.recv() {
            Ok(response) => {
                match response {
                    EvmApiOutcome::Bytes32(data, gas_cost) => {
                        (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::TrieSlots(gas_cost)=> {                        
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::TransientBytes32(data, gas_cost) => {
                        (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountBalance(data, gas_cost) => {
                        (data.to_be_bytes_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountCode(data, gas_cost) => {
                        (data.to_vec(), VecReader::new(vec![]), Gas(gas_cost))
                    },
                    EvmApiOutcome::AccountCodeHash(data, gas_cost) => {
                        (Status::Success.into(), VecReader::new(data.to_be_bytes_vec()), Gas(gas_cost))
                    },
                    EvmApiOutcome::HostIO(data, gas_cost) => {
                        (Status::Success.into(), VecReader::new(vec![]), Gas(gas_cost))
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
    last_request: Option<(EvmApiRequest, u32)>,
}

impl CothreadHandler {
    pub fn wait_next_message(&mut self) -> MaybeEscape {
        let msg = self.rx.recv_timeout(Duration::from_secs(10));
        let Ok(msg) = msg else {
            return MaybeEscape::Err(Escape::Exit(1));
        };
        self.last_request = Some((msg, 0x11111)); // TODO: Ids
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

    pub fn last_message(&self) -> Result<(EvmApiRequest, u32), Escape> {
        self.last_request
            .clone()
            .ok_or_else(|| Escape::Exit(4))
    }

    pub fn set_response(
        &mut self,
        id: u32,
        result: EvmApiOutcome
    ) -> MaybeEscape {
        let Some(msg) = self.last_request.clone() else {
            return MaybeEscape::Err(Escape::Exit(5));
        };
        if msg.1 != id {
            return MaybeEscape::Err(Escape::Exit(6));
        };
        
        if let Err(_) = self.tx.send(result) {
            return MaybeEscape::Err(Escape::Exit(7));
        };
        Ok(())
    }
}

/// Executes a wasm on a new thread
pub fn exec_wasm(
    module: &str,
    calldata: Vec<u8>,
    config: StylusConfig,
    evm_data: EvmData,
    ink: Ink,
) -> Result<CothreadHandler> {
    let (tothread_tx, tothread_rx) = mpsc::sync_channel::<EvmApiOutcome>(0);
    let (fromthread_tx, fromthread_rx) = mpsc::sync_channel::<EvmApiRequest>(0);

    let cothread = StylusRequestor {
        tx: fromthread_tx,
        rx: tothread_rx,
    };

    let evm_api = EvmApiRequestor::new(cothread);

    let instance = NativeInstance::from_path(
        module, 
        evm_api, 
        evm_data,
        &CompileConfig::default(),
        config,
        wasmer_types::compilation::target::Target::default(),

    );

    let mut instance = match instance {
        Ok(instance) => instance,
        Err(error) => Err(eyre!("failed to deserialize instance: {error}"))?,
    };

    let thread = thread::spawn(move || {
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

    Ok(CothreadHandler {
        tx: tothread_tx,
        rx: fromthread_rx,
        thread: Some(thread),
        last_request: None,
    })
}
