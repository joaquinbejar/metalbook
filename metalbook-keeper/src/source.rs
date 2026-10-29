//! Where the keeper gets Pyth prints.

use crate::KeeperError;

/// One Pyth `PriceFeedMessage`, as the keeper needs it to post a `PriceUpdateV2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrintMsg {
    /// Price mantissa.
    pub price: i64,
    /// Confidence mantissa.
    pub conf: u64,
    /// Decimal exponent.
    pub expo: i32,
    /// Publish time, seconds.
    pub publish_time: i64,
    /// Previous successful aggregate's publish time, seconds.
    pub prev_publish_time: i64,
}

/// A source of Pyth prints for one feed. Every query returns only prints already published
/// (`publish_time <= now`), and at most the one canonical print the query defines.
pub trait PriceSource {
    /// The canonical print of boundary `t`: `0 < prev_publish_time < t <= publish_time <=
    /// t + tolerance`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` if the source cannot answer.
    fn canonical_at(
        &self,
        feed: &[u8; 32],
        t: i64,
        tolerance: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError>;

    /// The canonical update that strictly advances past `t`: `0 < prev_publish_time <= t <
    /// publish_time`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` if the source cannot answer.
    fn next_after(
        &self,
        feed: &[u8; 32],
        t: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError>;

    /// The latest first-of-its-second print (`0 < prev_publish_time < publish_time`) with
    /// `earliest <= publish_time <= t`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` if the source cannot answer.
    fn latest_at_or_before(
        &self,
        feed: &[u8; 32],
        t: i64,
        earliest: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError>;
}

/// A recorded tape of one feed's messages, in publication order: synthetic in tests, or a
/// capture replayed later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapeSource {
    feed: [u8; 32],
    messages: Vec<PrintMsg>,
}

impl TapeSource {
    /// A tape of `messages` for `feed`.
    #[must_use]
    pub fn new(feed: [u8; 32], messages: Vec<PrintMsg>) -> Self {
        Self { feed, messages }
    }

    fn published<'a>(
        &'a self,
        feed: &[u8; 32],
        now: i64,
    ) -> impl Iterator<Item = &'a PrintMsg> + 'a {
        let same = feed == &self.feed;
        self.messages
            .iter()
            .filter(move |m| same && m.publish_time <= now && m.prev_publish_time > 0)
    }
}

impl PriceSource for TapeSource {
    fn canonical_at(
        &self,
        feed: &[u8; 32],
        t: i64,
        tolerance: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        let latest = t
            .checked_add(tolerance)
            .ok_or(KeeperError::Config("tolerance overflow"))?;
        Ok(self
            .published(feed, now)
            .find(|m| m.prev_publish_time < t && t <= m.publish_time && m.publish_time <= latest)
            .copied())
    }

    fn next_after(
        &self,
        feed: &[u8; 32],
        t: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        Ok(self
            .published(feed, now)
            .find(|m| m.prev_publish_time <= t && t < m.publish_time)
            .copied())
    }

    fn latest_at_or_before(
        &self,
        feed: &[u8; 32],
        t: i64,
        earliest: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        Ok(self
            .published(feed, now)
            .filter(|m| {
                m.prev_publish_time < m.publish_time
                    && m.publish_time >= earliest
                    && m.publish_time <= t
            })
            .max_by_key(|m| m.publish_time)
            .copied())
    }
}
