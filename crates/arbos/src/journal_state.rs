

use core::hash::Hash;
use core::mem;
use std::collections::HashMap;

use precompile::Log;
use revm::primitives::bytes::Bytes;
use revm::{context_interface::JournaledState as JournaledStateTrait, state::EvmState, Database, JournaledState};
use revm::primitives::Address;

use std::vec::Vec;

/// Journal entries that are used to track changes to the state and are used to revert it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WasmJournalEntry {
    Activated{ address: Address },
}

pub trait ArbOsJournalStateGetter {
    fn get_wasm(&self, address: Address) -> Option<&Bytes>;
}

/// A journal of state changes internal to the EVM.
///
/// On each additional call, the depth of the journaled state is increased (`depth`) and a new journal is added. The journal contains every state change that happens within that call, making it possible to revert changes made in a specific call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArbOsJournaledState<DB>  {
    inner: JournaledState<DB>,
    /// The journal of state changes, one for each call.
    pub wasm_journal: Vec<Vec<WasmJournalEntry>>,

    pub wasm_cache: HashMap<Address, Bytes>,
}

impl<DB: Database> ArbOsJournaledState<DB> {
    /// Revert all changes that happened in given journal entries.
    #[inline]
    fn journal_revert(
        journal_entries: Vec<WasmJournalEntry>
    ) {
        for entry in journal_entries.into_iter().rev() {
            match entry {
                WasmJournalEntry::Activated{ address } => {
                    // wasm_cache
                },
            }
        }
    }
}


impl<DB: Database> JournaledStateTrait for ArbOsJournaledState<DB>  {

    type Database = DB;
    // TODO make a struck here.
    type FinalOutput = (EvmState, Vec<Log>);

    fn warm_account_and_storage(
        &mut self,
        address: precompile::Address,
        storage_keys: impl IntoIterator<Item = revm::primitives::U256>,
    ) -> Result<(), <Self::Database as Database>::Error> {
        self.inner.warm_account_and_storage(address, storage_keys)
    }

    fn warm_account(&mut self, address: precompile::Address) {
        self.inner.warm_account(address)
    }

    fn set_spec_id(&mut self, spec_id: revm::specification::hardfork::SpecId) {
        self.inner.set_spec_id(spec_id)
    }

    fn touch_account(&mut self, address: precompile::Address) {
        self.inner.touch_account(address)
    }

    fn transfer(
        &mut self,
        from: &precompile::Address,
        to: &precompile::Address,
        balance: revm::primitives::U256,
    ) -> Result<Option<revm::context_interface::journaled_state::TransferError>, <Self::Database as Database>::Error> {
        self.inner.transfer(from, to, balance)
    }

    fn inc_account_nonce(
        &mut self,
        address: precompile::Address,
    ) -> Result<Option<u64>, <Self::Database as Database>::Error> {
        self.inner.inc_account_nonce(address)
    }

    fn load_account(
        &mut self,
        address: precompile::Address,
    ) -> Result<revm::interpreter::StateLoad<&mut revm::state::Account>, <Self::Database as Database>::Error> {
        self.inner.load_account(address)
    }

    fn load_account_code(
        &mut self,
        address: precompile::Address,
    ) -> Result<revm::interpreter::StateLoad<&mut revm::state::Account>, <Self::Database as Database>::Error> {
        self.inner.load_account_code(address)
    }

    fn load_account_delegated(
        &mut self,
        address: precompile::Address,
    ) -> Result<revm::context_interface::journaled_state::AccountLoad, <Self::Database as Database>::Error> {
        self.inner.load_account_delegated(address)
    }

    fn set_code_with_hash(&mut self, address: precompile::Address, code: revm::state::Bytecode, hash: precompile::B256) {
        self.inner.set_code_with_hash(address, code, hash)
    }

    fn clear(&mut self) {
        self.inner.clear()
    }

    fn checkpoint(&mut self) -> revm::context_interface::journaled_state::JournalCheckpoint {
        self.wasm_journal.push(Default::default());
        self.inner.checkpoint()
    }

    fn checkpoint_commit(&mut self) {
        self.inner.checkpoint_commit()
    }

    fn checkpoint_revert(&mut self, checkpoint: revm::context_interface::journaled_state::JournalCheckpoint) {
        let leng = self.wasm_journal.len();

        self.wasm_journal
        .iter_mut()
        .rev()
        .take(leng - checkpoint.journal_i)
        .for_each(|cs| {
            Self::journal_revert(
                mem::take(cs),
            )
        });
        self.inner.checkpoint_revert(checkpoint)
    }

    fn create_account_checkpoint(
        &mut self,
        caller: precompile::Address,
        address: precompile::Address,
        balance: revm::primitives::U256,
        spec_id: revm::specification::hardfork::SpecId,
    ) -> Result<revm::context_interface::journaled_state::JournalCheckpoint, revm::context_interface::journaled_state::TransferError> {
        self.inner.create_account_checkpoint(caller, address, balance, spec_id)
    }

    fn finalize(&mut self) -> Result<Self::FinalOutput, <Self::Database as Database>::Error> {
        self.inner.finalize()
    }

}

