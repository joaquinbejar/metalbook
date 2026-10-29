//! What the keeper needs from the chain.

use crate::{KeeperError, PrintMsg};

/// The keeper's view of the chain for one deployment: an RPC client in production, LiteSVM in
/// tests. `Address` is the client's account address type.
pub trait Chain {
    /// The client's account address type.
    type Address: Copy;

    /// Current cluster time (Clock sysvar), seconds.
    ///
    /// # Errors
    ///
    /// `KeeperError::Chain` if the clock cannot be read.
    fn now(&self) -> Result<i64, KeeperError>;

    /// The data of `account`, or `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// `KeeperError::Chain` if the account cannot be read.
    fn account_data(&self, account: &Self::Address) -> Result<Option<Vec<u8>>, KeeperError>;

    /// Makes `print` available as a `PriceUpdateV2` for `feed` (in production: post the Hermes
    /// update through the Pyth receiver) and calls `sample_settlement` on `series` with it.
    ///
    /// # Errors
    ///
    /// `KeeperError::Chain` if the transaction fails.
    fn post(
        &mut self,
        series: &Self::Address,
        feed: &[u8; 32],
        print: &PrintMsg,
    ) -> Result<(), KeeperError>;

    /// Calls `settle` on `series`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Chain` if the transaction fails.
    fn settle(&mut self, series: &Self::Address) -> Result<(), KeeperError>;
}
