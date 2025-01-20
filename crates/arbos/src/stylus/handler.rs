use std::mem;

use arbutil::evm::{
    api::{EvmApiMethod, Gas, VecReader},
    req::RequestHandler,
};
use revm::{handler::FrameResult, interpreter::Host};
use revm::{
    interpreter::{CallInputs, FrameInput},
    primitives::{Address, Log},
};

use super::revm_types;

pub struct StylusHandler<CTX: 'static> {
    pub address: Address,
    pub api: &'static mut CTX,
    pub new_frame_cb: FrameCreateFunc<CTX>,
    pub is_static: bool,
}

unsafe impl<CTX> Send for StylusHandler<CTX> {}

pub type FrameCreateFunc<CTX> = Box<dyn FnMut(&mut CTX, FrameInput) -> FrameResult>;

impl<CTX: Host + Send + 'static> StylusHandler<CTX> {
    pub fn new(
        context: &mut CTX,
        address: Address,
        cb: FrameCreateFunc<CTX>,
        is_static: bool,
    ) -> Self {
        let unsafe_context: &'static mut CTX = unsafe { mem::transmute(context) };

        Self {
            address,
            api: unsafe_context,
            new_frame_cb: cb,
            is_static,
        }
    }
}

impl<CTX> RequestHandler<VecReader> for StylusHandler<CTX>
where
    CTX: Host + Send + 'static,
{
    fn request(
        &mut self,
        req_type: arbutil::evm::api::EvmApiMethod,
        req_data: impl AsRef<[u8]>,
    ) -> (Vec<u8>, VecReader, arbutil::evm::api::Gas) {
        let mut data = req_data.as_ref().to_vec();
        match req_type {
            EvmApiMethod::GetBytes32 => {
                let slot = revm_types::take_u256(&mut data);
                if let Some(result) = self.api.sload(self.address, slot) {
                    (result.to_be_bytes_vec(), VecReader::new(vec![]), Gas(0))
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::SetTrieSlots => {
                if self.is_static {
                    return (
                        Status::WriteProtection.into(),
                        VecReader::new(vec![]),
                        Gas(0),
                    );
                }

                let gas_left = revm_types::take_u64(&mut data);
                let key = revm_types::take_u256(&mut data);
                let value = revm_types::take_u256(&mut data);
                if self.api.sstore(self.address, key, value).is_some() {
                    (Status::Success.into(), VecReader::new(vec![]), Gas(0))
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::GetTransientBytes32 => {
                let slot = revm_types::take_u256(&mut data);
                let result = self.api.tload(self.address, slot);
                (result.to_be_bytes_vec(), VecReader::new(vec![]), Gas(0))
            }
            EvmApiMethod::SetTransientBytes32 => {
                if self.is_static {
                    return (
                        Status::WriteProtection.into(),
                        VecReader::new(vec![]),
                        Gas(0),
                    );
                }

                let key = revm_types::take_u256(&mut data);
                let value = revm_types::take_u256(&mut data);
                self.api.tstore(self.address, key, value);
                (Status::Success.into(), VecReader::new(vec![]), Gas(0))
            }
            EvmApiMethod::ContractCall | EvmApiMethod::DelegateCall | EvmApiMethod::StaticCall => {
                let address = revm_types::take_address(&mut data);
                let value = revm_types::take_u256(&mut data);
                let _ = revm_types::take_u64(&mut data);
                let gas_limit = revm_types::take_u64(&mut data);
                let calldata = revm_types::take_rest(&mut data);

                let res = (self.new_frame_cb)(
                    self.api,
                    FrameInput::Call(Box::new(CallInputs {
                        input: calldata,
                        return_memory_offset: 0..0,
                        gas_limit,
                        bytecode_address: address,
                        target_address: address,
                        caller: self.address,
                        value: revm::interpreter::CallValue::Transfer(value),
                        scheme: revm::interpreter::CallScheme::Call,
                        is_static: self.is_static,
                        is_eof: false,
                    })),
                );

                if let FrameResult::Call(result) = res {
                    (
                        Status::Success.into(),
                        VecReader::new(result.result.output.to_vec()),
                        Gas(0),
                    )
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::Create1 => {
                let gas_limit = revm_types::take_u64(&mut data);
                let value = revm_types::take_u256(&mut data);
                let code = revm_types::take_rest(&mut data);

                todo!();
            }
            EvmApiMethod::Create2 => {
                if self.is_static {
                    return (
                        Status::WriteProtection.into(),
                        VecReader::new(vec![]),
                        Gas(0),
                    );
                }

                let gas_limit = revm_types::take_u64(&mut data);
                let value = revm_types::take_u256(&mut data);
                let salt = revm_types::take_bytes32(&mut data);
                let code = revm_types::take_rest(&mut data);

                todo!();
            }
            EvmApiMethod::EmitLog => {
                if self.is_static {
                    return (
                        Status::WriteProtection.into(),
                        VecReader::new(vec![]),
                        Gas(0),
                    );
                }

                let topic_count = revm_types::take_u32(&mut data);

                let mut topics = vec![];

                for _ in 0..topic_count {
                    let hash = revm_types::take_bytes32(&mut data);
                    topics.push(hash);
                }

                let data = revm_types::take_rest(&mut data);

                let log = Log::new_unchecked(self.address, topics, data);

                self.api.log(log);

                (Status::Success.into(), VecReader::new(vec![]), Gas(0))
            }
            EvmApiMethod::AccountBalance => {
                let address = revm_types::take_address(&mut data);
                if let Some(balance) = self.api.balance(address) {
                    (balance.to_be_bytes_vec(), VecReader::new(vec![]), Gas(0))
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::AccountCode => {
                let address = revm_types::take_address(&mut data);
                if let Some(code) = self.api.code(address) {
                    (
                        Status::Success.into(),
                        VecReader::new(code.to_vec()),
                        Gas(0),
                    )
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::AccountCodeHash => {
                let address = revm_types::take_address(&mut data);
                if let Some(code_hash) = self.api.code_hash(address) {
                    (code_hash.to_vec(), VecReader::new(vec![]), Gas(0))
                } else {
                    (vec![], VecReader::new(vec![]), Gas(0))
                }
            }
            EvmApiMethod::AddPages => {
                let count = revm_types::take_u16(&mut data);
                (Status::Success.into(), VecReader::new(vec![]), Gas(0))
            }
            EvmApiMethod::CaptureHostIO => (Status::Success.into(), VecReader::new(vec![]), Gas(0)),
        }
    }
}

enum Status {
    Success,
    Failure,
    OutOfGas,
    WriteProtection,
}

impl From<Status> for Vec<u8> {
    fn from(status: Status) -> Vec<u8> {
        match status {
            Status::Success => vec![0],
            Status::Failure => vec![1],
            Status::OutOfGas => vec![2],
            Status::WriteProtection => vec![3],
        }
    }
}
