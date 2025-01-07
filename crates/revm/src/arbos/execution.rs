
use std::sync::mpsc::{self, Receiver, SyncSender};

use crate::{
    arbos::requestor::{exec_wasm, EvmApiOutcome, EvmApiRequest},
    db::Database,
    interpreter::{Gas, InstructionResult},
    primitives::EVMError,
    Context, Frame,
};

use arbutil::{
    evm::EvmData,
    Bytes20, Bytes32,
};
use revm_interpreter::{
    CallInputs, CallOutcome, CallScheme, CallValue, InterpreterAction, InterpreterResult, SharedMemory
};
use stylus::prover::programs::config::StylusConfig;

use crate::primitives::U256;
use crate::primitives::{address, Address, Bytes};

use super::requestor::StylusOutcome;

pub struct StylusFrame {
    evm_data: EvmData,
    tx: SyncSender<EvmApiOutcome>,
    rx: Receiver<EvmApiRequest>,
    pub frame: Frame,
}

pub fn is_stylus_bytecode(address: Address) -> bool {
    address == address!("0d4a11d5eeaac28ec3f61d100daf4d40471f1852")
}

impl StylusFrame {
    pub fn make_call_frame(frame: Frame) -> Self {
        let config = StylusConfig::default();

        let mut evm_data = EvmData::default();
        evm_data.msg_sender =
            Bytes20::try_from(frame.interpreter().contract.caller.as_slice()).unwrap();
        evm_data.msg_value =
            Bytes32::try_from(frame.interpreter().contract.call_value.to_be_bytes_vec()).unwrap();
        //evm_data.tx_gas_price = Bytes32::try_from(inputs.).unwrap();
        evm_data.contract_address =
            Bytes20::try_from(frame.interpreter().contract.target_address.as_slice()).unwrap();

        let module = if frame.interpreter().contract.target_address
            == address!("0d4a11d5eeaac28ec3f61d100daf4d40471f1852")
        {
            "./stylus_hello_world.wasm"
        } else {
            todo!("Invalid bytecode address");
        };

        let (tothread_tx, tothread_rx) = mpsc::sync_channel::<EvmApiOutcome>(0);
        let (fromthread_tx, fromthread_rx) = mpsc::sync_channel::<EvmApiRequest>(0);

        exec_wasm(
            module,
            frame.interpreter().contract.input.to_vec(),
            config,
            evm_data,
            frame.interpreter().gas,
            fromthread_tx,
            tothread_rx,
        );

        Self {
            tx: tothread_tx,
            rx: fromthread_rx,
            evm_data,
            frame,
        }
    }

    fn next_action(
        &self,
        mut handler: impl FnMut(EvmApiRequest) -> EvmApiOutcome,
    ) -> InterpreterAction {
        loop {
            let request = self.rx.recv().unwrap();

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
                    self.tx.send(outcome).unwrap();
                }
                EvmApiRequest::ContractCall(call_arguments) => {
                    let caller = Address::try_from(self.evm_data.msg_sender.as_slice()).unwrap();

                    let action = InterpreterAction::Call {
                        inputs: Box::new(CallInputs {
                            input: call_arguments.calldata,
                            return_memory_offset: 0..0,
                            gas_limit: call_arguments.gas_limit,
                            bytecode_address: call_arguments.address,
                            target_address: call_arguments.address,
                            caller: caller,
                            value: CallValue::Transfer(call_arguments.value),
                            scheme: CallScheme::Call,
                            is_static: self.frame.interpreter().is_static,
                            is_eof: false,
                        }),
                    };

                    return action;
                }
                EvmApiRequest::DelegateCall(..) | EvmApiRequest::StaticCall(..) => {
                    todo!()
                },
                EvmApiRequest::Create1(..) | EvmApiRequest::Create2(..) => todo!(),
                EvmApiRequest::Return(outcome, _) => {
                    return InterpreterAction::Return { result: outcome.result}
                }
            };
        }
    }

    pub fn handle_return(&self, result: EvmApiOutcome) {
        self.tx.send(result).unwrap();
        //println!("sent result");
    }

}

/// Execute frame
#[inline]
pub fn stylus_execute_frame<EXT, DB: Database>(
    frame: &mut StylusFrame,
    context: &mut Context<EXT, DB>,
) -> Result<InterpreterAction, EVMError<DB::Error>> {
    let address = Address::from_slice(frame.evm_data.contract_address.as_slice());
    let handler = |request| match request {
        EvmApiRequest::GetBytes32(slot) => {
            let value = if let Ok(value) = context.evm.sload(address, slot) {
                value.data
            } else {
                U256::ZERO
            };

            EvmApiOutcome::GetBytes32(value, 0)
        }
        EvmApiRequest::AddPages(_) => EvmApiOutcome::AddPages(0),
        EvmApiRequest::SetTrieSlots(key , value , data, gas_left) => {
            _ = context.evm.sstore(address, key.into(), value.into());
            EvmApiOutcome::SetTrieSlots(0)
        }
        _ => todo!(),
    };

    Ok(frame.next_action(handler))
}

#[inline]
pub fn stylus_insert_call_outcome<EXT, DB: Database>(
    context: &mut Context<EXT, DB>,
    frame: &mut StylusFrame,
    outcome: CallOutcome,
) -> Result<(), EVMError<DB::Error>> {
    
    let outcome = outcome.result;
    match outcome.result {
        InstructionResult::Return => {
            frame.handle_return(EvmApiOutcome::Call(StylusOutcome::Return((outcome.output)), 0));
        }
        InstructionResult::Revert => {
            frame.handle_return(EvmApiOutcome::Call(StylusOutcome::Revert((outcome.output)), 0));
        }
        InstructionResult::FatalExternalError => {
            frame.handle_return(EvmApiOutcome::Call(StylusOutcome::Failure, 0));
        }
        InstructionResult::OutOfGas => {
            frame.handle_return(EvmApiOutcome::Call(StylusOutcome::OutOfInk, 0));
        }
        _ => unreachable!("Invalid instruction result"),
    }
    Ok(())
}
