use crate::{
    arbos::requestor::{exec_wasm, EvmApiRequest, MessageFromCothread}, db::Database, interpreter::{
        Gas, InstructionResult,
        SharedMemory,
    }, primitives::{EVMError, Spec}, Context, Frame
};

use arbutil::evm::{api::{EvmApiMethod, EVM_API_METHOD_REQ_OFFSET}, EvmData};
use revm_interpreter::{
    opcode::InstructionTables, Host, InterpreterAction, InterpreterResult
};
use stylus::prover::programs::config::StylusConfig;
use crate::primitives::U256;
use crate::primitives::hex;


pub fn execute_stylus_frame<SPEC: Spec, EXT, DB: Database>(
    frame: &mut Frame,
    shared_memory: &mut SharedMemory,
    context: &mut Context<EXT, DB>,
) -> Result<InterpreterAction, EVMError<DB::Error>> {
    println!("Executing wasm module");
    
    let config = StylusConfig::default();

    let mut handler = exec_wasm(
        "./erc20.wasm",
        hex!("70a082310000000000000000000000000000000000000000000000000000000000000000").to_vec(),
        config,
        EvmData::default(),
        config.pricing.gas_to_ink(arbutil::evm::api::Gas(frame.interpreter().gas().remaining())),
    ).unwrap();


    loop {
        handler.wait_next_message().unwrap();
        
        let (request, gas_left) = handler.last_message().unwrap();

        match request {
            EvmApiRequest::GetBytes32(slot) => {                      
                todo!()
            },
            EvmApiRequest::SetTrieSlots(address, key, data, gas) => todo!(),
            EvmApiRequest::GetTransientBytes32(slot) => todo!(),
            EvmApiRequest::SetTransientBytes32(slot) => todo!(),
            EvmApiRequest::ContractCall(inputs) => todo!(),
            EvmApiRequest::DelegateCall(inputs) => todo!(),
            EvmApiRequest::StaticCall(inputs) => todo!(),
            EvmApiRequest::Create1(inputs) => todo!(),
            EvmApiRequest::Create2(inputs) => todo!(),
            EvmApiRequest::EmitLog(log) => todo!(),
            EvmApiRequest::AccountBalance(address) => todo!(),
            EvmApiRequest::AccountCode(address) => todo!(),
            EvmApiRequest::AccountCodeHash(address) => todo!(),
            EvmApiRequest::AddPages(count) => {
                todo!()
            },
            EvmApiRequest::CaptureHostIO => todo!(),
            EvmApiRequest::StylusOutcome(outcome) => {
                // Assume successful return
                let _ = handler.set_response(id, vec![0; 32], Vec::new(), arbutil::evm::api::Gas(0));

                let gas_left: u64 = u64::from_be_bytes(req_data[0..8].try_into().unwrap());
                let data = req_data[8..].to_vec();

                let next_action = InterpreterAction::Return { result: InterpreterResult { result: InstructionResult::Return, output: revm_precompile::Bytes::from(data), gas: Gas::new(gas_left) } };

                let interpreter = frame.interpreter_mut();
                *shared_memory = interpreter.take_memory();
                return Ok(next_action);
            }
        }
        
        //let result = handler.set_response(id, Vec::new(), Vec::new(), arbutil::evm::api::Gas(0));
        //if let Ok(_) = result {
           // break;
        //}
        
    }
}

