
use revm::{
    context::{block::BlockEnv, tx::TxEnv, CfgEnv, Context},
    context_interface::result::{EVMError, InvalidTransaction},
    database_interface::Database,
    Evm,
};

use crate::handler::ArbOsHandler;

/// ArbOS Error.
pub type ArbOsError<DB> = EVMError<<DB as Database>::Error, InvalidTransaction>;

/// ArbOS Context.
pub type ArbOsContext<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB>;

/// ArbOS EVM type.
pub type ArbOsEvm<DB> =
    Evm<ArbOsError<DB>, ArbOsContext<DB>, ArbOsHandler<ArbOsContext<DB>, ArbOsError<DB>>>;

// pub type InspCtxType<INSP, DB> =
//     InspectorContext<INSP, BlockEnv, TxEnv, CfgEnv, DB>;

// pub type InspectorArbOsEvm<DB, INSP> = Evm<
//     ArbOsError<DB>,
//     InspCtxType<INSP, DB>,
//     ArbOsHandler<
//         InspCtxType<INSP, DB>,
//         ArbOsError<DB>,
//         EthValidation<InspCtxType<INSP, DB>, ArbOsError<DB>>,
//         EthPreExecution<InspCtxType<INSP, DB>, ArbOsError<DB>>,
//         ArbOsExecution<
//             InspCtxType<INSP, DB>,
//             ArbOsError<DB>,
//             InspectorEthFrame<
//                 InspCtxType<INSP, DB>,
//                 ArbOsError<DB>,
//                 EthPrecompileProvider<InspCtxType<INSP, DB>, ArbOsError<DB>>,
//             >,
//         >,
//     >,
// >;
