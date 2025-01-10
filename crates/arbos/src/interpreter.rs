use revm::interpreter::{
    interpreter::EthInterpreter, Interpreter,
};

use crate::stylus::interpreter::StylusInterpreter;

pub enum EthOrStylusInterpreter {
    Eth(Box<Interpreter<EthInterpreter>>),
    Stylus(Box<StylusInterpreter>),
}
