use revm::{context_interface::{BlockGetter, CfgGetter, ErrorGetter, JournalStateGetter, TransactionGetter}, interpreter::Host};

pub trait StylusFrameContext<ERROR>:
    TransactionGetter + Host + ErrorGetter<Error = ERROR> + BlockGetter + JournalStateGetter + CfgGetter
{
}
