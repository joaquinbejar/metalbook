//! What the keeper needs from the chain.

use crate::{BatchError, KeeperError, PrintMsg};

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

    /// How many series one [`Chain::post_batch`] of `print` carries: the most for which the
    /// transaction that samples them stays within the size and compute limits. The default is
    /// 1 (no batching).
    ///
    /// # Errors
    ///
    /// `KeeperError` if it cannot be computed (the update cannot be fetched or planned).
    fn batch_capacity(&self, feed: &[u8; 32], print: &PrintMsg) -> Result<usize, KeeperError> {
        let _ = (feed, print);
        Ok(1)
    }

    /// Makes `print` available once and samples it into every series of `series` (at most
    /// [`Chain::batch_capacity`]). The default posts once per series, which is not atomic: a
    /// failure part way leaves the earlier ones stored, which the caller finds by re-reading.
    ///
    /// # Errors
    ///
    /// [`BatchError::Uncertain`] if the post may still land, [`BatchError::Failed`] otherwise.
    fn post_batch(
        &mut self,
        series: &[Self::Address],
        feed: &[u8; 32],
        print: &PrintMsg,
    ) -> Result<(), BatchError> {
        for s in series {
            self.post(s, feed, print).map_err(BatchError::Failed)?;
        }
        Ok(())
    }

    /// Calls `settle` on every series of `series`. The default settles one by one.
    ///
    /// # Errors
    ///
    /// `KeeperError::Chain` on the first failure; the caller re-reads to see which landed.
    fn settle_batch(&mut self, series: &[Self::Address]) -> Result<(), KeeperError> {
        for s in series {
            self.settle(s)?;
        }
        Ok(())
    }
}
