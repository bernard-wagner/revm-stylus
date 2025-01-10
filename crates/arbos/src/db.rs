use database::{CacheDB, DbAccount};
use revm::context_interface::DBErrorMarker;
use revm::database_interface::{Database, DatabaseCommit, DatabaseRef, EmptyDB};

use revm::primitives::{Address, HashMap, B256, U256};
use revm::state::{Account, AccountInfo, Bytecode};

/// A [Database] implementation that stores all state changes in memory.
pub type InMemoryDB = StylusCacheDB<EmptyDB>;

/// A [Database] implementation that stores all state changes in memory.
///
/// This implementation wraps a [DatabaseRef] that is used to load data ([AccountInfo]).
///
/// Accounts and code are stored in two separate maps, the `accounts` map maps addresses to [DbAccount],
/// whereas contracts are identified by their code hash, and are stored in the `contracts` map.
/// The [DbAccount] holds the code hash of the contract, which is used to look up the contract in the `contracts` map.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StylusCacheDB<ExtDB> {
    inner: CacheDB<ExtDB>,
    wasms: HashMap<Address, (Bytecode, bool)>,
}

pub trait WasmDatabase<ExtDB> {
    /// The database error type.
    type Error: DBErrorMarker;

    fn insert_wasm(&mut self, address: Address, code: Bytecode, is_activated: bool);

    fn load_wasm(&mut self, address: Address) -> Result<(Bytecode, bool), Self::Error>;
}

impl<ExtDB: Database> WasmDatabase<ExtDB> for StylusCacheDB<ExtDB> 
where <ExtDB as revm::Database>::Error: From<&'static str> {
    type Error = ExtDB::Error;

    fn insert_wasm(&mut self, address: Address, code: Bytecode, is_activated: bool) {
        self.wasms.insert(address, (code, is_activated));
    }

    fn load_wasm(&mut self, address: Address) -> Result<(Bytecode, bool), Self::Error> {
        self.wasms
            .get(&address)
            .map(|(code, is_activated)| (code.clone(), *is_activated))
            .ok_or_else(|| "Wasm not found".into())
    }
}

impl<ExtDB> StylusCacheDB<ExtDB> {
    /// Create a new cache with the given external database.
    pub fn new(db: ExtDB) -> Self {
        Self {
            inner: CacheDB::new(db),
            wasms: HashMap::new(),
        }
    }

    /// Inserts the account's code into the cache.
    ///
    /// Accounts objects and code are stored separately in the cache, this will take the code from the account and instead map it to the code hash.
    ///
    /// Note: This will not insert into the underlying external database.
    pub fn insert_contract(&mut self, account: &mut AccountInfo) {
        self.inner.insert_contract(account);
    }

    /// Insert account info but not override storage
    pub fn insert_account_info(&mut self, address: Address, info: AccountInfo) {
        self.inner.insert_account_info(address, info);
    }

    pub fn insert_wasm(&mut self, address: Address, code: Bytecode, is_activated: bool) {
        self.wasms.insert(address, (code, is_activated));
    }
}

impl<ExtDB: DatabaseRef> StylusCacheDB<ExtDB> {
    /// Returns the account for the given address.
    ///
    /// If the account was not found in the cache, it will be loaded from the underlying database.
    pub fn load_account(&mut self, address: Address) -> Result<&mut DbAccount, ExtDB::Error> {
        self.inner.load_account(address)
    }

    /// insert account storage without overriding account info
    pub fn insert_account_storage(
        &mut self,
        address: Address,
        slot: U256,
        value: U256,
    ) -> Result<(), ExtDB::Error> {
        self.inner.insert_account_storage(address, slot, value)
    }

    /// replace account storage without overriding account info
    pub fn replace_account_storage(
        &mut self,
        address: Address,
        storage: HashMap<U256, U256>,
    ) -> Result<(), ExtDB::Error> {
        self.inner.replace_account_storage(address, storage)
    }
}

impl<ExtDB> DatabaseCommit for StylusCacheDB<ExtDB> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        self.inner.commit(changes)
    }
}

impl<ExtDB: DatabaseRef> Database for StylusCacheDB<ExtDB> {
    type Error = ExtDB::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash(code_hash)
    }

    /// Get the value in an account's storage slot.
    ///
    /// It is assumed that account is already loaded.
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash(number)
    }
}

impl<ExtDB: DatabaseRef> DatabaseRef for StylusCacheDB<ExtDB> {
    type Error = ExtDB::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic_ref(address)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash_ref(code_hash)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage_ref(address, index)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash_ref(number)
    }
}
