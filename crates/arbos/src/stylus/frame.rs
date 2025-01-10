use core::cell::RefCell;
use std::{rc::Rc, sync::mpsc::{self, Receiver, SyncSender}};

use arbutil::{evm::EvmData, Bytes20, Bytes32};
use revm::{context::Cfg, context_interface::{journaled_state::JournalCheckpoint, transaction::{CommonTxFields, Eip1559CommonTxFields, Eip2930Tx}, Block, BlockGetter, CfgGetter, DatabaseGetter, JournalStateGetter, JournaledState, Transaction, TransactionGetter}, handler::{EthFrame, EthFrameContext, EthFrameError, EthPrecompileProvider, FrameResult}, handler_interface::{Frame, FrameOrResultGen, PrecompileProvider}, interpreter::{interpreter::{EthInstructionProvider, EthInterpreter, InstructionProvider}, CallInputs, CallScheme, CallValue, FrameInput, Host, InterpreterAction, InterpreterTypes, SharedMemory}, primitives::{bytes, keccak256, U256}, state::Bytecode};
use revm::primitives::Address;
use stylus::prover::programs::config::StylusConfig;
use crate::context::StylusFrameContext;

use super::handler::{exec_wasm, stylus_call, EvmApiOutcome, EvmApiRequest};


pub struct StylusFrame<CTX, ERROR, IW: InterpreterTypes, PRECOMPILE, INSTRUCTION> {
    eth: EthFrame<CTX, ERROR, IW, PRECOMPILE, INSTRUCTION>,
    depth: usize,
    tx: SyncSender<EvmApiOutcome>,
    rx: Receiver<EvmApiRequest>,
    
}

fn build_evm_data<CTX: Host + JournalStateGetter>(context: &mut CTX, inputs: &CallInputs) -> EvmData 
{    
    let block = context.block();
    let tx = context.tx();
    let base_fee = block.basefee();
    
    let evm_data = EvmData {
        arbos_version: 0,
        block_basefee: Bytes32::try_from(block.basefee().to_be_bytes_vec()).unwrap(),
        chainid: context.cfg().chain_id(),
        block_coinbase: Bytes20::try_from(block.beneficiary().as_slice()).unwrap(),
        block_gas_limit: block.gas_limit().to::<u64>(),
        block_number: block.number().to::<u64>(),
        block_timestamp: block.timestamp().to::<u64>(),
        contract_address: Bytes20::try_from(inputs.bytecode_address.as_slice()).unwrap(),
        module_hash: Bytes32::try_from(inputs.bytecode_address.as_slice()).unwrap(),
        msg_sender: Bytes20::try_from(inputs.caller.as_slice()).unwrap(),
        msg_value: Bytes32::try_from(inputs.value.get().to_be_bytes_vec()).unwrap(),
        tx_gas_price: Bytes32::from(tx.effective_gas_price(*base_fee).to_be_bytes()),
        tx_origin: Bytes20::try_from(tx.common_fields().caller().as_slice()).unwrap(),
        reentrant: 0,
        return_data_len: 0,
        cached: false,
        tracing: false,
    };

    evm_data
}

impl<CTX, ERROR, PRECOMPILE, INSTRUCTION> StylusFrame<CTX, ERROR, EthInterpreter<()>, PRECOMPILE, INSTRUCTION>
where
    CTX: EthFrameContext<ERROR>,
    ERROR: EthFrameError<CTX>,
    PRECOMPILE: PrecompileProvider<Context = CTX, Error = ERROR>,
    INSTRUCTION: InstructionProvider<WIRE = EthInterpreter<()>, Host = CTX>,
    {

    /// Make call frame
    #[inline]
    pub fn make_call_frame(
        context: &mut CTX,
        depth: usize,
        memory: Rc<RefCell<SharedMemory>>,
        inputs: &CallInputs,
        bytecode: Bytecode,
        mut precompile: PRECOMPILE,
        instructions: INSTRUCTION,
    ) -> Result<FrameOrResultGen<Self, FrameResult>, ERROR> {
        let eth_frame = match EthFrame::make_call_frame(context, depth, memory, inputs, precompile, instructions)? {
            FrameOrResultGen::Frame(frame) => frame,
            _ => todo!(),
        };

        let (tothread_tx, tothread_rx) = mpsc::sync_channel::<EvmApiOutcome>(0);
        let (fromthread_tx, fromthread_rx) = mpsc::sync_channel::<EvmApiRequest>(0);

        let evm_data = build_evm_data(context, inputs);

        let gas_limit = inputs.gas_limit;

        stylus_call(bytecode.bytes().into(), inputs.input.clone(), StylusConfig::default(), evm_data, gas_limit, false, fromthread_tx, tothread_rx);

        Ok(FrameOrResultGen::Frame(Self {
            eth: eth_frame,
            depth: depth,
            tx: tothread_tx,
            rx: fromthread_rx,
        }))

    }
    

    fn next_action(
        &self,
        mut handler: impl FnMut(EvmApiRequest) -> EvmApiOutcome,
    ) ->  InterpreterAction {
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
                    let caller = Address::try_from(self.eth.interpreter.input.target_address.as_slice()).unwrap();                    
                    return InterpreterAction::NewFrame(FrameInput::Call(Box::new(CallInputs {
                        input: call_arguments.calldata,
                        return_memory_offset: 0..0,
                        gas_limit: call_arguments.gas_limit,
                        bytecode_address: call_arguments.address,
                        target_address: call_arguments.address,
                        caller: caller,
                        value: CallValue::Transfer(call_arguments.value),
                        scheme: CallScheme::Call,
                        is_static: false,
                        is_eof: false,
                    })));
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

    fn run(
        &mut self,
        context: &mut CTX,
    ) -> Result<FrameOrResultGen<Self::FrameInit, Self::FrameResult>, Self::Error>  {
     
        let handler = |request| match request {
            EvmApiRequest::GetBytes32(slot) => {
                let address = Address::from_slice(self.eth.interpreter.input.target_address.as_slice());

                let value = if let Some(value) = context.sload(address, slot) {
                    value.data
                } else {
                    U256::ZERO
                };

                EvmApiOutcome::GetBytes32(value, 0)
            }
            EvmApiRequest::AddPages(_) => EvmApiOutcome::AddPages(0),
            EvmApiRequest::SetTrieSlots(key , value , data, gas_left) => {
                let address = Address::from_slice(self.eth.interpreter.input.target_address.as_slice());

                _ = context.sstore(address, key.into(), value.into());
                EvmApiOutcome::SetTrieSlots(0)
            }
            _ => todo!(),
    
        };
    
        let interpreter_result = self.next_action(handler);

        match interpreter_result {
            InterpreterAction::NewFrame(frame_input) => {
                Ok(FrameOrResultGen::Frame(frame_input))
            },
            InterpreterAction::Return { result } => {
                Ok(FrameOrResultGen::Result(result))
            },
            InterpreterAction::None => todo!(),
        }
    
    }

    pub fn depth(&self) -> usize {
        self.eth.depth()
    }
}


