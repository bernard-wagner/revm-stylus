use core::mem;
use std::sync::mpsc::{Receiver, SyncSender};

use crate::{
    arbos::requestor::{exec_wasm, EvmApiOutcome, EvmApiRequest, MessageFromCothread}, db::Database, interpreter::{
        Gas, InstructionResult,
        SharedMemory,
    }, primitives::{EVMError, Spec}, Context, Frame
};

use arbutil::evm::{api::{EvmApiMethod, EVM_API_METHOD_REQ_OFFSET}, EvmData};
use revm_interpreter::{
    opcode::InstructionTables, Host, InterpreterAction, InterpreterResult, EMPTY_SHARED_MEMORY
};
use stylus::prover::programs::config::StylusConfig;
use crate::primitives::U256;
use crate::primitives::hex;


pub fn execute_stylus_frame(
    frame: &mut Frame,
    tx: SyncSender<EvmApiRequest>,
    rx: Receiver<EvmApiOutcome>,  
) {
    println!("Executing wasm module");
    
    let config = StylusConfig::default();

   exec_wasm(
        "./erc20.wasm",
        hex!("70a082310000000000000000000000000000000000000000000000000000000000000000").to_vec(),
        config,
        EvmData::default(),
        config.pricing.gas_to_ink(arbutil::evm::api::Gas(frame.interpreter().gas().remaining())),
        tx,
        rx,
    );
}
