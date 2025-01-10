//! Handler related to ArbOs chain
use revm::{
    context_interface::result::HaltReason,
    handler::{
        EthExecution,
        EthHandler, EthPostExecution, EthPreExecution, EthPrecompileProvider,
        EthValidation,
    },
    interpreter::interpreter::{EthInstructionProvider, EthInterpreter},
};

use crate::frame::ArbOsFrame;

pub type ArbOsExecution<
    CTX,
    ERROR,
    FRAME = ArbOsFrame<
        CTX,
        ERROR,
        EthPrecompileProvider<CTX, ERROR>,
        EthInstructionProvider<EthInterpreter<()>, CTX>,
    >,
> = EthExecution<CTX, ERROR, FRAME>;

pub type ArbOsHandler<
    CTX,
    ERROR,
    VAL = EthValidation<CTX, ERROR>,
    PREEXEC = EthPreExecution<CTX, ERROR>,
    EXEC = ArbOsExecution<CTX, ERROR>,
    POSTEXEC = EthPostExecution<CTX, ERROR, HaltReason>,
> = EthHandler<CTX, ERROR, VAL, PREEXEC, EXEC, POSTEXEC>;
