//! Settlement price capture (`docs/PLAN.md` 4.2 and 4.3): canonical window slots, the
//! canonical next-print chain, the fallback candidate, and the settlement cascade.
//!
//! Each candidate value is canonical, so whoever posts a print has no choice of price *for
//! that slot, link or fallback*:
//! - a window slot `i` takes the canonical print of its boundary `t_i`:
//!   `prev_publish_time < t_i <= publish_time <= t_i + slot_tolerance_s`;
//! - the next-print chain takes the canonical update that strictly advances past
//!   `window_end`, then past each link in turn;
//! - the fallback is the latest first-of-its-second print at or before expiry.
//!
//! *Which* candidates are stored when `settle` runs does depend on what was posted: an
//! adversary alone can post a subset of the window or an older fallback and settle on it.
//! Every candidate is therefore monotonically improvable (more slots, a longer chain, a later
//! fallback). An improvement restarts that candidate's challenge delay `D`, except that a
//! complete window settles at once and a complete chain from `window_end + D`, since neither
//! leaves a subset to choose. Correctness assumes at least one honest keeper submits, within
//! `D` of any change, every available candidate of higher priority than the one that would
//! otherwise settle: all canonical window slots, all chain links and the latest fallback
//! (`docs/PLAN.md` 4.3, trust assumption).
//!
//! Prices are raw Pyth mantissas with the series' pinned exponent; they are converted to
//! USDC atoms per ounce once, when the settlement price is fixed.

use crate::{CoreError, StrikeUsd, UnixTimestamp};

/// Largest number of window slots (the slot bitmap is a `u32`).
pub const MAX_SLOTS: u32 = 32;

/// Capacity of the next-print chain.
pub const MAX_NEXT_PRINTS: usize = 5;

/// Smallest accepted Pyth exponent.
pub const MIN_EXPO: i8 = -12;

/// Largest accepted Pyth exponent.
pub const MAX_EXPO: i8 = 0;

/// Decimals of USDC: settlement prices are in USDC atoms per ounce.
const USDC_DECIMALS: i32 = 6;

/// The oracle terms a series copies at creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OracleParams {
    window_s: u32,
    slot_s: u32,
    slot_tolerance_s: u16,
    min_samples: u16,
    k_next_print: u8,
    challenge_delay_s: u32,
    fallback_lookback_s: u32,
    hard_deadline_s: u32,
    expo: i8,
}

impl OracleParams {
    /// Checks and builds the oracle terms.
    ///
    /// # Errors
    ///
    /// `InvalidOracleParams` unless: `slot_s > 0` divides `window_s`, the window has 1 to
    /// [`MAX_SLOTS`] slots, `slot_tolerance_s < slot_s`, `1 <= min_samples <= slots`,
    /// `1 <= k_next_print <= MAX_NEXT_PRINTS`, `challenge_delay_s > 0`,
    /// `fallback_lookback_s > 0`, `hard_deadline_s > window_s` and
    /// `MIN_EXPO <= expo <= MAX_EXPO`.
    #[allow(clippy::too_many_arguments)]
    #[must_use = "the result carries a possible error"]
    pub fn try_new(
        window_s: u32,
        slot_s: u32,
        slot_tolerance_s: u16,
        min_samples: u16,
        k_next_print: u8,
        challenge_delay_s: u32,
        fallback_lookback_s: u32,
        hard_deadline_s: u32,
        expo: i8,
    ) -> Result<Self, CoreError> {
        let bad = CoreError::InvalidOracleParams;
        if slot_s == 0 || window_s.checked_rem(slot_s) != Some(0) {
            return Err(bad);
        }
        let slots = window_s.checked_div(slot_s).ok_or(bad)?;
        let valid = (1..=MAX_SLOTS).contains(&slots)
            && u32::from(slot_tolerance_s) < slot_s
            && min_samples >= 1
            && u32::from(min_samples) <= slots
            && k_next_print >= 1
            && usize::from(k_next_print) <= MAX_NEXT_PRINTS
            && challenge_delay_s > 0
            && fallback_lookback_s > 0
            && hard_deadline_s > window_s
            && (MIN_EXPO..=MAX_EXPO).contains(&expo);
        if !valid {
            return Err(bad);
        }
        Ok(Self {
            window_s,
            slot_s,
            slot_tolerance_s,
            min_samples,
            k_next_print,
            challenge_delay_s,
            fallback_lookback_s,
            hard_deadline_s,
            expo,
        })
    }

    /// Length of the averaging window before expiry, seconds.
    #[must_use]
    pub const fn window_s(&self) -> u32 {
        self.window_s
    }

    /// Spacing of the window slots, seconds.
    #[must_use]
    pub const fn slot_s(&self) -> u32 {
        self.slot_s
    }

    /// How late after its boundary a canonical slot print may be published, seconds.
    #[must_use]
    pub const fn slot_tolerance_s(&self) -> u16 {
        self.slot_tolerance_s
    }

    /// Filled slots needed for a `Window` settlement before every slot is filled.
    #[must_use]
    pub const fn min_samples(&self) -> u16 {
        self.min_samples
    }

    /// Links of a complete next-print chain.
    #[must_use]
    pub const fn k_next_print(&self) -> u8 {
        self.k_next_print
    }

    /// Challenge delay `D`, seconds.
    #[must_use]
    pub const fn challenge_delay_s(&self) -> u32 {
        self.challenge_delay_s
    }

    /// How far before expiry a fallback print may be, seconds.
    #[must_use]
    pub const fn fallback_lookback_s(&self) -> u32 {
        self.fallback_lookback_s
    }

    /// Hard deadline after expiry, seconds.
    #[must_use]
    pub const fn hard_deadline_s(&self) -> u32 {
        self.hard_deadline_s
    }

    /// Pinned Pyth exponent.
    #[must_use]
    pub const fn expo(&self) -> i8 {
        self.expo
    }

    /// Number of window slots.
    #[must_use]
    pub const fn slots(&self) -> u32 {
        // `try_new` proved `slot_s > 0` and that it divides `window_s`.
        match self.window_s.checked_div(self.slot_s) {
            Some(n) => n,
            None => 0,
        }
    }
}

/// A Pyth print that passed the oracle checks (owner, `Full`, feed, exponent, confidence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Print {
    price: u64,
    publish_time: i64,
    prev_publish_time: i64,
}

impl Print {
    /// Checks and builds a print: price mantissa (with the pinned exponent), publish time and
    /// the previous aggregate's publish time, both in seconds.
    ///
    /// # Errors
    ///
    /// `InvalidPrint` for a zero price, `prev_publish_time <= 0` (the first aggregate after a
    /// feed's (re)initialization has `0`, and its interval would overlap earlier prints,
    /// leaving a choice), or `prev_publish_time > publish_time`.
    #[must_use = "the result carries a possible error"]
    pub fn try_new(
        price: u64,
        publish_time: i64,
        prev_publish_time: i64,
    ) -> Result<Self, CoreError> {
        if price == 0 || prev_publish_time <= 0 || prev_publish_time > publish_time {
            return Err(CoreError::InvalidPrint);
        }
        Ok(Self {
            price,
            publish_time,
            prev_publish_time,
        })
    }

    /// Price mantissa.
    #[must_use]
    pub const fn price(&self) -> u64 {
        self.price
    }

    /// Publish time, seconds.
    #[must_use]
    pub const fn publish_time(&self) -> i64 {
        self.publish_time
    }

    /// Previous aggregate's publish time, seconds.
    #[must_use]
    pub const fn prev_publish_time(&self) -> i64 {
        self.prev_publish_time
    }
}

/// Which candidates a print was stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrintUse {
    /// The window slot it filled.
    pub slot: Option<u8>,
    /// The next-print link it became (0-based).
    pub next_print: Option<u8>,
    /// Whether it became the fallback candidate.
    pub fallback: bool,
}

/// How a settlement price was obtained (cascade of `docs/PLAN.md` 4.3), stored as `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SettleMethod {
    /// Mean of at least `min_samples` window slots.
    Window = 0,
    /// Mean of a complete next-print chain.
    NextPrint = 1,
    /// Mean of a partial next-print chain, after the hard deadline.
    PartialNextPrint = 2,
    /// Mean of fewer than `min_samples` slots, after the hard deadline.
    PartialWindow = 3,
    /// The fallback print, after the hard deadline.
    Fallback = 4,
    /// No price: the series is voided and writers recover their collateral.
    Void = 5,
}

impl TryFrom<u8> for SettleMethod {
    type Error = CoreError;

    /// # Errors
    ///
    /// `InvalidDiscriminant` for any value above 5.
    fn try_from(value: u8) -> Result<Self, CoreError> {
        match value {
            0 => Ok(Self::Window),
            1 => Ok(Self::NextPrint),
            2 => Ok(Self::PartialNextPrint),
            3 => Ok(Self::PartialWindow),
            4 => Ok(Self::Fallback),
            5 => Ok(Self::Void),
            _ => Err(CoreError::InvalidDiscriminant),
        }
    }
}

/// Outcome of [`SettlementState::decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Settle at `price` (USDC atoms per ounce), obtained by `method` from `samples` prints.
    Settle {
        /// The cascade step used.
        method: SettleMethod,
        /// Settlement price, USDC atoms per ounce, rounded to nearest (half up).
        price: StrikeUsd,
        /// Number of prints averaged.
        samples: u16,
    },
    /// Nothing was ever posted by the hard deadline (plus `D`): void the series.
    Void,
    /// A candidate is pending, or nothing is eligible yet: try again from `until`.
    Wait {
        /// Earliest time at which the decision can change without new prints.
        until: i64,
    },
}

/// The settlement candidates of one series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettlementState {
    sample_sum: u128,
    filled_slots: u32,
    next_times: [i64; MAX_NEXT_PRINTS],
    next_prices: [u64; MAX_NEXT_PRINTS],
    next_len: u8,
    fallback_time: i64,
    fallback_price: u64,
    window_changed: i64,
    next_print_changed: i64,
    fallback_changed: i64,
}

/// The stored form of a [`SettlementState`]. A plain carrier: values read from an account are
/// untrusted until [`SettlementState::restore`] accepts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettlementParts {
    /// Sum of the raw mantissas of the filled slots.
    pub sample_sum: u128,
    /// Bitmap of filled slots (bit `i` is slot `i`).
    pub filled_slots: u32,
    /// Publish times of the next-print links; zero past `next_len`.
    pub next_times: [i64; MAX_NEXT_PRINTS],
    /// Raw mantissas of the next-print links; zero past `next_len`.
    pub next_prices: [u64; MAX_NEXT_PRINTS],
    /// Number of links.
    pub next_len: u8,
    /// Publish time of the fallback candidate (zero if none).
    pub fallback_time: i64,
    /// Raw mantissa of the fallback candidate (zero if none).
    pub fallback_price: u64,
    /// Time of the last new slot.
    pub window_changed: i64,
    /// Time of the last new link.
    pub next_print_changed: i64,
    /// Time of the last fallback replacement.
    pub fallback_changed: i64,
}

impl SettlementState {
    /// Rebuilds a state from stored fields, checking that it is reachable for `params`:
    /// slots inside the window, a chain no longer than `k` with strictly increasing times and
    /// positive prices, zeros after the chain, a fallback that is either empty or has a
    /// positive price, and no slot sum without slots.
    ///
    /// # Errors
    ///
    /// `InvariantViolated` otherwise.
    #[must_use = "the result carries a possible error"]
    pub fn restore(params: &OracleParams, p: SettlementParts) -> Result<Self, CoreError> {
        let bad = CoreError::InvariantViolated;
        let slots = params.slots();
        let mask_ok = slots >= MAX_SLOTS || p.filled_slots.checked_shr(slots).unwrap_or(0) == 0;
        let sum_ok = (p.filled_slots == 0) == (p.sample_sum == 0);
        let len = usize::from(p.next_len);
        let len_ok = p.next_len <= params.k_next_print();
        let mut chain_ok = true;
        let mut previous: Option<i64> = None;
        for (i, (&t, &px)) in p.next_times.iter().zip(p.next_prices.iter()).enumerate() {
            if i < len {
                chain_ok &= px > 0 && previous.is_none_or(|q| t > q);
                previous = Some(t);
            } else {
                chain_ok &= t == 0 && px == 0;
            }
        }
        let fallback_ok = (p.fallback_time == 0) == (p.fallback_price == 0);
        if !(mask_ok && sum_ok && len_ok && chain_ok && fallback_ok) {
            return Err(bad);
        }
        Ok(Self {
            sample_sum: p.sample_sum,
            filled_slots: p.filled_slots,
            next_times: p.next_times,
            next_prices: p.next_prices,
            next_len: p.next_len,
            fallback_time: p.fallback_time,
            fallback_price: p.fallback_price,
            window_changed: p.window_changed,
            next_print_changed: p.next_print_changed,
            fallback_changed: p.fallback_changed,
        })
    }

    /// The fields to store.
    #[must_use]
    pub const fn parts(&self) -> SettlementParts {
        SettlementParts {
            sample_sum: self.sample_sum,
            filled_slots: self.filled_slots,
            next_times: self.next_times,
            next_prices: self.next_prices,
            next_len: self.next_len,
            fallback_time: self.fallback_time,
            fallback_price: self.fallback_price,
            window_changed: self.window_changed,
            next_print_changed: self.next_print_changed,
            fallback_changed: self.fallback_changed,
        }
    }

    /// Number of filled window slots.
    #[must_use]
    pub const fn filled(&self) -> u32 {
        self.filled_slots.count_ones()
    }

    /// Number of next-print links.
    #[must_use]
    pub const fn next_len(&self) -> u8 {
        self.next_len
    }

    /// Stores `print` as every candidate it qualifies for, at time `now`. A print may fill a
    /// window slot and replace the fallback at once; the next-print chain only takes prints
    /// after `expiry`, which no slot or fallback can.
    ///
    /// # Errors
    ///
    /// `PrintNotUsable` if the print qualifies for nothing (outside every range, not canonical,
    /// or already stored), `Overflow` on time arithmetic.
    pub fn add_print(
        &mut self,
        params: &OracleParams,
        expiry: UnixTimestamp,
        print: Print,
        now: i64,
    ) -> Result<PrintUse, CoreError> {
        let mut next = *self;
        let used = PrintUse {
            slot: next.try_slot(params, expiry, &print, now)?,
            next_print: next.try_next_print(params, expiry, &print, now),
            fallback: next.try_fallback(params, expiry, &print, now)?,
        };
        if used.slot.is_none() && used.next_print.is_none() && !used.fallback {
            return Err(CoreError::PrintNotUsable);
        }
        *self = next;
        Ok(used)
    }

    /// Canonical slot of `print`, if it is one and the slot is empty.
    fn try_slot(
        &mut self,
        params: &OracleParams,
        expiry: UnixTimestamp,
        print: &Print,
        now: i64,
    ) -> Result<Option<u8>, CoreError> {
        let window_start = expiry
            .get()
            .checked_sub(i64::from(params.window_s()))
            .ok_or(CoreError::Overflow)?;
        let Some(offset) = print.publish_time.checked_sub(window_start) else {
            return Ok(None);
        };
        if offset < 0 {
            return Ok(None);
        }
        let slot_s = i64::from(params.slot_s());
        let index = offset.checked_div(slot_s).ok_or(CoreError::Overflow)?;
        if index >= i64::from(params.slots()) {
            return Ok(None);
        }
        let boundary = window_start
            .checked_add(index.checked_mul(slot_s).ok_or(CoreError::Overflow)?)
            .ok_or(CoreError::Overflow)?;
        let late = print
            .publish_time
            .checked_sub(boundary)
            .ok_or(CoreError::Overflow)?;
        // `boundary <= publish_time` holds by construction of `index`.
        if late > i64::from(params.slot_tolerance_s()) || print.prev_publish_time >= boundary {
            return Ok(None);
        }
        let index = u32::try_from(index).map_err(|_| CoreError::Overflow)?;
        let bit = 1u32.checked_shl(index).ok_or(CoreError::Overflow)?;
        if self.filled_slots & bit != 0 {
            return Ok(None);
        }
        self.filled_slots |= bit;
        self.sample_sum = self
            .sample_sum
            .checked_add(u128::from(print.price))
            .ok_or(CoreError::Overflow)?;
        self.window_changed = now;
        Ok(Some(u8::try_from(index).map_err(|_| CoreError::Overflow)?))
    }

    /// Next link of the canonical chain, if `print` is it.
    fn try_next_print(
        &mut self,
        params: &OracleParams,
        expiry: UnixTimestamp,
        print: &Print,
        now: i64,
    ) -> Option<u8> {
        if self.next_len >= params.k_next_print() {
            return None;
        }
        let len = usize::from(self.next_len);
        let links = match len.checked_sub(1) {
            // First link: the canonical update that strictly advances past `window_end`.
            None => print.prev_publish_time <= expiry.get() && expiry.get() < print.publish_time,
            // Later links: chained on the last one and strictly advancing (a failed
            // aggregation repeats `prev_publish_time == publish_time` and must not match).
            Some(last) => {
                let last_time = *self.next_times.get(last)?;
                print.prev_publish_time == last_time && print.publish_time > last_time
            }
        };
        if !links {
            return None;
        }
        *self.next_times.get_mut(len)? = print.publish_time;
        *self.next_prices.get_mut(len)? = print.price;
        let index = self.next_len;
        self.next_len = self.next_len.checked_add(1)?;
        self.next_print_changed = now;
        Some(index)
    }

    /// Replaces the fallback if `print` is a later first-of-its-second print in
    /// `[expiry − fallback_lookback_s, expiry]`.
    fn try_fallback(
        &mut self,
        params: &OracleParams,
        expiry: UnixTimestamp,
        print: &Print,
        now: i64,
    ) -> Result<bool, CoreError> {
        let earliest = expiry
            .get()
            .checked_sub(i64::from(params.fallback_lookback_s()))
            .ok_or(CoreError::Overflow)?;
        // `prev < publish` keeps one print per second: later aggregates of the same second
        // repeat `prev_publish_time == publish_time`, so no same-second choice remains.
        let qualifies = print.publish_time >= earliest
            && print.publish_time <= expiry.get()
            && print.prev_publish_time < print.publish_time
            && print.publish_time > self.fallback_time;
        if !qualifies {
            return Ok(false);
        }
        self.fallback_time = print.publish_time;
        self.fallback_price = print.price;
        self.fallback_changed = now;
        Ok(true)
    }

    /// The settlement cascade at time `now` (`docs/PLAN.md` 4.3). Steps run in priority
    /// order; a candidate that exists but is still inside its challenge delay is pending, not
    /// absent, so the cascade waits instead of falling through to a lower step.
    ///
    /// 1. `Window`: every slot filled, from `window_end`; or at least `min_samples`, from
    ///    `max(window_end, window_changed) + D`.
    /// 2. `NextPrint`: a complete chain, from `window_end + D` (so the window always gets
    ///    `D` to be backfilled first).
    /// 3. From the hard deadline: a partial chain from `max(hard, next_print_changed) + D`,
    ///    else a partial window from `max(hard, window_changed) + D`.
    /// 4. `Fallback` from `max(hard, fallback_changed) + D`.
    /// 5. Void from `hard + D` if nothing was ever stored.
    ///
    /// # Errors
    ///
    /// `Overflow` on time or price arithmetic.
    pub fn decide(
        &self,
        params: &OracleParams,
        expiry: UnixTimestamp,
        now: i64,
    ) -> Result<Decision, CoreError> {
        let window_end = expiry.get();
        if now < window_end {
            return Ok(Decision::Wait { until: window_end });
        }
        let d = i64::from(params.challenge_delay_s());
        let after = |t: i64| t.checked_add(d).ok_or(CoreError::Overflow);
        let hard = window_end
            .checked_add(i64::from(params.hard_deadline_s()))
            .ok_or(CoreError::Overflow)?;
        let filled = self.filled();
        let expo = params.expo();

        // 1. Window.
        if filled >= params.slots() {
            return self.window(expo, SettleMethod::Window);
        }
        if filled >= u32::from(params.min_samples()) {
            let until = after(window_end.max(self.window_changed))?;
            return if now >= until {
                self.window(expo, SettleMethod::Window)
            } else {
                Ok(Decision::Wait { until })
            };
        }
        // 2. Complete next-print chain.
        if self.next_len >= params.k_next_print() {
            let until = after(window_end)?;
            return if now >= until {
                self.chain(expo, SettleMethod::NextPrint)
            } else {
                Ok(Decision::Wait { until })
            };
        }
        // 3. to 5. only from the hard deadline.
        if now < hard {
            return Ok(Decision::Wait { until: hard });
        }
        if self.next_len > 0 {
            let until = after(hard.max(self.next_print_changed))?;
            return if now >= until {
                self.chain(expo, SettleMethod::PartialNextPrint)
            } else {
                Ok(Decision::Wait { until })
            };
        }
        if filled > 0 {
            let until = after(hard.max(self.window_changed))?;
            return if now >= until {
                self.window(expo, SettleMethod::PartialWindow)
            } else {
                Ok(Decision::Wait { until })
            };
        }
        if self.fallback_price > 0 {
            let until = after(hard.max(self.fallback_changed))?;
            return if now >= until {
                Ok(Decision::Settle {
                    method: SettleMethod::Fallback,
                    price: to_atoms(u128::from(self.fallback_price), 1, expo)?,
                    samples: 1,
                })
            } else {
                Ok(Decision::Wait { until })
            };
        }
        let until = after(hard)?;
        if now >= until {
            Ok(Decision::Void)
        } else {
            Ok(Decision::Wait { until })
        }
    }

    fn window(&self, expo: i8, method: SettleMethod) -> Result<Decision, CoreError> {
        let n = self.filled();
        Ok(Decision::Settle {
            method,
            price: to_atoms(self.sample_sum, n, expo)?,
            samples: u16::try_from(n).map_err(|_| CoreError::Overflow)?,
        })
    }

    fn chain(&self, expo: i8, method: SettleMethod) -> Result<Decision, CoreError> {
        let mut sum: u128 = 0;
        for &p in self.next_prices.iter().take(usize::from(self.next_len)) {
            sum = sum.checked_add(u128::from(p)).ok_or(CoreError::Overflow)?;
        }
        Ok(Decision::Settle {
            method,
            price: to_atoms(sum, u32::from(self.next_len), expo)?,
            samples: u16::from(self.next_len),
        })
    }
}

/// `sum × 10^expo / count` in USDC atoms (6 decimals), rounded to nearest, ties up.
///
/// One rounding for the whole mean. Rounding to nearest is neutral: the settlement price is a
/// fixing that moves value between longs and writers, not out of the vault, and the payout
/// derived from it is still rounded down toward the protocol.
///
/// # Errors
///
/// `Overflow` if `count` is zero or the price does not fit a `u64`.
pub fn to_atoms(sum: u128, count: u32, expo: i8) -> Result<StrikeUsd, CoreError> {
    let shift = USDC_DECIMALS
        .checked_add(i32::from(expo))
        .ok_or(CoreError::Overflow)?;
    let scale = 10u128
        .checked_pow(shift.unsigned_abs())
        .ok_or(CoreError::Overflow)?;
    let (num, den) = if shift >= 0 {
        (
            sum.checked_mul(scale).ok_or(CoreError::Overflow)?,
            u128::from(count),
        )
    } else {
        (
            sum,
            u128::from(count)
                .checked_mul(scale)
                .ok_or(CoreError::Overflow)?,
        )
    };
    let q = num.checked_div(den).ok_or(CoreError::Overflow)?;
    let r = num.checked_rem(den).ok_or(CoreError::Overflow)?;
    let twice = r.checked_mul(2).ok_or(CoreError::Overflow)?;
    let rounded = if twice >= den {
        q.checked_add(1).ok_or(CoreError::Overflow)?
    } else {
        q
    };
    u64::try_from(rounded)
        .map(StrikeUsd::new)
        .map_err(|_| CoreError::Overflow)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::field_reassign_with_default,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    const EXPIRY: i64 = 1_800_000_000;

    fn params() -> OracleParams {
        // 30 slots of 60 s, tolerance 10 s, 20 samples, k = 5, D = 600 s, X = 24 h,
        // hard deadline 7 days, XAU exponent -3.
        OracleParams::try_new(1_800, 60, 10, 20, 5, 600, 86_400, 604_800, -3).unwrap()
    }

    fn print(price: u64, publish: i64, prev: i64) -> Print {
        Print::try_new(price, publish, prev).unwrap()
    }

    fn e() -> UnixTimestamp {
        UnixTimestamp::new(EXPIRY)
    }

    #[test]
    fn test_oracle_params_validation() {
        let ok = |w, s, tol, min, k, d, x, h, expo| {
            OracleParams::try_new(w, s, tol, min, k, d, x, h, expo).is_ok()
        };
        assert!(ok(1_800, 60, 10, 20, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 0, 10, 20, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 70, 10, 20, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(3_300, 100, 10, 20, 5, 600, 86_400, 604_800, -3)); // 33 slots
        assert!(!ok(1_800, 60, 60, 20, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 31, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 0, 5, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 6, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 0, 600, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 5, 0, 86_400, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 5, 600, 0, 604_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 5, 600, 86_400, 1_800, -3));
        assert!(!ok(1_800, 60, 10, 20, 5, 600, 86_400, 604_800, 1));
        assert!(!ok(1_800, 60, 10, 20, 5, 600, 86_400, 604_800, -13));
    }

    #[test]
    fn test_print_rejects_zero_price_and_prev_zero() {
        assert_eq!(Print::try_new(0, 10, 9), Err(CoreError::InvalidPrint));
        assert_eq!(Print::try_new(1, 10, 0), Err(CoreError::InvalidPrint));
        assert_eq!(Print::try_new(1, 10, 11), Err(CoreError::InvalidPrint));
        assert!(Print::try_new(1, 10, 10).is_ok());
    }

    #[test]
    fn test_slot_canonical_rules() {
        let p = params();
        let start = EXPIRY - 1_800;
        let mut s = SettlementState::default();
        // Canonical for slot 3: prev < t_3 <= publish <= t_3 + 10.
        let t3 = start + 180;
        assert_eq!(
            s.add_print(&p, e(), print(4_000_000, t3 + 2, t3 - 1), 1)
                .unwrap()
                .slot,
            Some(3)
        );
        // Same slot again: already filled; not usable for anything else (fallback takes
        // only later prints, and this one is not later).
        assert_eq!(
            s.add_print(&p, e(), print(4_000_000, t3 + 2, t3 - 1), 2),
            Err(CoreError::PrintNotUsable)
        );
        // Too late after the boundary.
        let t4 = start + 240;
        let used = s
            .add_print(&p, e(), print(4_000_000, t4 + 11, t4 - 1), 3)
            .unwrap();
        assert_eq!(used.slot, None);
        // Not crossing the boundary (prev >= t_5).
        let t5 = start + 300;
        let used = s
            .add_print(&p, e(), print(4_000_000, t5 + 3, t5), 4)
            .unwrap();
        assert_eq!(used.slot, None);
        // Same-second repeat (prev == publish) never fills a slot.
        let t6 = start + 360;
        assert_eq!(
            s.add_print(&p, e(), print(4_000_000, t6, t6), 5),
            Err(CoreError::PrintNotUsable)
        );
        assert_eq!(s.filled(), 1);
    }

    #[test]
    fn test_print_after_gap_fills_at_most_one_slot() {
        let p = params();
        let start = EXPIRY - 1_800;
        let mut s = SettlementState::default();
        // A print 20 minutes after the previous one: its interval covers 20 boundaries, but
        // only the boundary it is within tolerance of can take it.
        let publish = start + 1_200 + 5;
        let used = s
            .add_print(&p, e(), print(1, publish, publish - 1_200), 1)
            .unwrap();
        assert_eq!(used.slot, Some(20));
        assert_eq!(s.filled(), 1);
    }

    #[test]
    fn test_next_print_chain_strictly_advances() {
        let p = params();
        let mut s = SettlementState::default();
        assert_eq!(
            s.add_print(&p, e(), print(10, EXPIRY + 1, EXPIRY - 1), 1)
                .unwrap()
                .next_print,
            Some(0)
        );
        // A non-advancing repeat (prev == publish == last link) is rejected.
        assert_eq!(
            s.add_print(&p, e(), print(11, EXPIRY + 1, EXPIRY + 1), 2),
            Err(CoreError::PrintNotUsable)
        );
        assert_eq!(s.next_len(), 1);
        // Two distinct messages with the same publish time: only the one chained on the last
        // link is accepted, in either order.
        assert_eq!(
            s.add_print(&p, e(), print(12, EXPIRY + 2, EXPIRY + 2), 3),
            Err(CoreError::PrintNotUsable)
        );
        assert_eq!(
            s.add_print(&p, e(), print(13, EXPIRY + 2, EXPIRY + 1), 4)
                .unwrap()
                .next_print,
            Some(1)
        );
        assert_eq!(
            s.add_print(&p, e(), print(12, EXPIRY + 2, EXPIRY + 2), 5),
            Err(CoreError::PrintNotUsable)
        );
        // A print that skips a link does not chain.
        assert_eq!(
            s.add_print(&p, e(), print(14, EXPIRY + 9, EXPIRY + 5), 6),
            Err(CoreError::PrintNotUsable)
        );
        assert_eq!(s.next_len(), 2);
    }

    #[test]
    fn test_fallback_keeps_latest_first_of_second_print() {
        let p = params();
        let mut s = SettlementState::default();
        // Before the window and inside the lookback.
        assert!(
            s.add_print(&p, e(), print(5, EXPIRY - 7_200, EXPIRY - 7_201), 1)
                .unwrap()
                .fallback
        );
        // Earlier: not an improvement.
        assert_eq!(
            s.add_print(&p, e(), print(6, EXPIRY - 7_300, EXPIRY - 7_301), 2),
            Err(CoreError::PrintNotUsable)
        );
        // Same-second repeat: not first of its second.
        assert_eq!(
            s.add_print(&p, e(), print(6, EXPIRY - 7_000, EXPIRY - 7_000), 3),
            Err(CoreError::PrintNotUsable)
        );
        // Outside the lookback.
        assert_eq!(
            s.add_print(&p, e(), print(6, EXPIRY - 90_000, EXPIRY - 90_001), 4),
            Err(CoreError::PrintNotUsable)
        );
        assert!(
            s.add_print(&p, e(), print(7, EXPIRY, EXPIRY - 1), 5)
                .unwrap()
                .fallback
        );
        assert_eq!(s.parts().fallback_price, 7);
    }

    #[test]
    fn test_to_atoms_rounds_to_nearest() {
        // XAU expo -3: 4_000_123 × 10^-3 USD = 4000.123 USD = 4_000_123_000 atoms.
        assert_eq!(
            to_atoms(4_000_123, 1, -3),
            Ok(StrikeUsd::new(4_000_123_000))
        );
        // Mean of 1 and 2 at expo -6: 1.5 atoms, ties up.
        assert_eq!(to_atoms(3, 2, -6), Ok(StrikeUsd::new(2)));
        // expo -8: 15 × 10^-8 USD = 0.15 atoms, rounds to 0; 50 → 0.5 → 1.
        assert_eq!(to_atoms(15, 1, -8), Ok(StrikeUsd::new(0)));
        assert_eq!(to_atoms(50, 1, -8), Ok(StrikeUsd::new(1)));
        assert_eq!(to_atoms(1, 0, -3), Err(CoreError::Overflow));
    }

    /// A state with `filled` slots, `links` chain links and an optional fallback, every
    /// change at time `changed`.
    fn state(filled: u32, links: u8, fallback: bool, changed: i64) -> SettlementState {
        let mut p = SettlementParts::default();
        for i in 0..filled {
            p.filled_slots |= 1 << i;
            p.sample_sum += 4_000_000;
        }
        for j in 0..usize::from(links) {
            p.next_times[j] = EXPIRY + 1 + j as i64;
            p.next_prices[j] = 4_100_000;
        }
        p.next_len = links;
        if fallback {
            p.fallback_time = EXPIRY - 1;
            p.fallback_price = 3_900_000;
        }
        p.window_changed = changed;
        p.next_print_changed = changed;
        p.fallback_changed = changed;
        SettlementState::restore(&params(), p).unwrap()
    }

    fn method(d: Decision) -> Option<SettleMethod> {
        match d {
            Decision::Settle { method, .. } => Some(method),
            Decision::Void => Some(SettleMethod::Void),
            Decision::Wait { .. } => None,
        }
    }

    #[test]
    fn test_cascade_order_and_delays() {
        let p = params();
        let hard = EXPIRY + 604_800;
        let d = |s: SettlementState, now: i64| s.decide(&p, e(), now).unwrap();
        // Nothing before expiry.
        assert_eq!(
            d(state(30, 5, true, 0), EXPIRY - 1),
            Decision::Wait { until: EXPIRY }
        );
        // A full window settles at window_end, even in the transaction that filled it.
        assert_eq!(
            method(d(state(30, 0, false, EXPIRY), EXPIRY)),
            Some(SettleMethod::Window)
        );
        // min_samples: waits D after the last new slot.
        let s = state(20, 5, true, EXPIRY + 100);
        assert_eq!(
            d(s, EXPIRY + 699),
            Decision::Wait {
                until: EXPIRY + 700
            }
        );
        assert_eq!(method(d(s, EXPIRY + 700)), Some(SettleMethod::Window));
        // A pending window blocks the complete chain (priority), even long after.
        let s = state(20, 5, true, hard + 50);
        assert_eq!(d(s, hard + 100), Decision::Wait { until: hard + 650 });
        // Complete chain: from window_end + D.
        let s = state(19, 5, true, EXPIRY + 5);
        assert_eq!(
            d(s, EXPIRY + 599),
            Decision::Wait {
                until: EXPIRY + 600
            }
        );
        assert_eq!(method(d(s, EXPIRY + 600)), Some(SettleMethod::NextPrint));
        // Partial candidates wait for the hard deadline.
        let s = state(19, 4, true, EXPIRY + 5);
        assert_eq!(d(s, hard - 1), Decision::Wait { until: hard });
        assert_eq!(d(s, hard), Decision::Wait { until: hard + 600 });
        assert_eq!(
            method(d(s, hard + 600)),
            Some(SettleMethod::PartialNextPrint)
        );
        // A partial chain changed after the deadline is pending and blocks the partial window
        // and the fallback.
        let s = state(19, 4, true, hard + 300);
        assert_eq!(d(s, hard + 600), Decision::Wait { until: hard + 900 });
        assert_eq!(
            method(d(state(19, 0, true, 0), hard + 600)),
            Some(SettleMethod::PartialWindow)
        );
        assert_eq!(
            method(d(state(0, 0, true, 0), hard + 600)),
            Some(SettleMethod::Fallback)
        );
        assert_eq!(
            d(state(0, 0, false, 0), hard + 599),
            Decision::Wait { until: hard + 600 }
        );
        assert_eq!(d(state(0, 0, false, 0), hard + 600), Decision::Void);
    }

    /// Codex's counterexample on #9, kept as a regression that documents the honest-keeper
    /// assumption: the fallback price depends on which valid print was posted. With only the
    /// older print (expiry − 3000 at 1000) the series settles at 1_000_000 atoms; with only
    /// the later one (expiry − 2000 at 2000) at 2_000_000. Both prints are valid; nothing but a
    /// keeper posting the later one within `D` prevents the first outcome.
    #[test]
    fn test_fallback_outcome_depends_on_what_was_posted() {
        let p = params();
        let hard = EXPIRY + 604_800;
        let settle_with = |px: u64, publish: i64| {
            let mut s = SettlementState::default();
            s.add_print(&p, e(), print(px, publish, publish - 1), hard)
                .unwrap();
            s.decide(&p, e(), hard + 600).unwrap()
        };
        let fallback = |atoms: u64| Decision::Settle {
            method: SettleMethod::Fallback,
            price: StrikeUsd::new(atoms),
            samples: 1,
        };
        assert_eq!(settle_with(1_000, EXPIRY - 3_000), fallback(1_000_000));
        assert_eq!(settle_with(2_000, EXPIRY - 2_000), fallback(2_000_000));
    }

    #[test]
    fn test_improvements_during_d_replace_and_restart_the_delay() {
        let p = params();
        let hard = EXPIRY + 604_800;
        // An adversary posts an older fallback after the hard deadline...
        let mut s = SettlementState::default();
        s.add_print(&p, e(), print(1_000, EXPIRY - 3_000, EXPIRY - 3_001), hard)
            .unwrap();
        // ...an honest keeper posts a later one during D: it replaces it and restarts D.
        s.add_print(
            &p,
            e(),
            print(2_000, EXPIRY - 2_000, EXPIRY - 2_001),
            hard + 500,
        )
        .unwrap();
        assert_eq!(
            s.decide(&p, e(), hard + 600),
            Ok(Decision::Wait {
                until: hard + 1_100
            })
        );
        assert_eq!(
            s.decide(&p, e(), hard + 1_100),
            Ok(Decision::Settle {
                method: SettleMethod::Fallback,
                price: StrikeUsd::new(2_000_000),
                samples: 1
            })
        );
        // An adversary posts 20 favorable slots; an honest keeper adds the other 10 during D.
        let start = EXPIRY - 1_800;
        let mut s = SettlementState::default();
        for i in 0..20 {
            let b = start + i * 60;
            s.add_print(&p, e(), print(4_400_000, b + 1, b - 1), EXPIRY + 10)
                .unwrap();
        }
        assert_eq!(
            s.decide(&p, e(), EXPIRY + 609),
            Ok(Decision::Wait {
                until: EXPIRY + 610
            })
        );
        for i in 20..29 {
            let b = start + i * 60;
            s.add_print(&p, e(), print(4_000_000, b + 1, b - 1), EXPIRY + 600)
                .unwrap();
        }
        // 29 slots: D restarted at the last addition.
        assert_eq!(
            s.decide(&p, e(), EXPIRY + 610),
            Ok(Decision::Wait {
                until: EXPIRY + 1_200
            })
        );
        let b = start + 29 * 60;
        s.add_print(&p, e(), print(4_000_000, b + 1, b - 1), EXPIRY + 650)
            .unwrap();
        // Complete: settles at once, on every slot (20 × 4400 + 10 × 4000) / 30.
        assert_eq!(
            s.decide(&p, e(), EXPIRY + 650),
            Ok(Decision::Settle {
                method: SettleMethod::Window,
                price: StrikeUsd::new(4_266_666_667),
                samples: 30
            })
        );
    }

    #[test]
    fn test_window_price_is_mean_in_atoms() {
        let p = params();
        let mut parts = SettlementParts::default();
        parts.filled_slots = 0b111;
        parts.sample_sum = 4_000_000 + 4_000_001 + 4_000_002; // XAU, expo -3
        let s = SettlementState::restore(&p, parts).unwrap();
        // Three slots < min_samples: only as a partial window after the hard deadline.
        let hard = EXPIRY + 604_800;
        assert_eq!(
            s.decide(&p, e(), hard + 600),
            Ok(Decision::Settle {
                method: SettleMethod::PartialWindow,
                price: StrikeUsd::new(4_000_001_000),
                samples: 3
            })
        );
    }

    #[test]
    fn test_restore_rejects_unreachable_states() {
        let p = params();
        let mut bad = SettlementParts::default();
        bad.filled_slots = 1 << 30; // slot 30 of 30 slots does not exist
        bad.sample_sum = 1;
        assert_eq!(
            SettlementState::restore(&p, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = SettlementParts::default();
        bad.sample_sum = 1;
        assert_eq!(
            SettlementState::restore(&p, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = SettlementParts::default();
        bad.next_len = 2;
        bad.next_times = [5, 5, 0, 0, 0];
        bad.next_prices = [1, 1, 0, 0, 0];
        assert_eq!(
            SettlementState::restore(&p, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = SettlementParts::default();
        bad.next_times = [0, 0, 0, 0, 7];
        assert_eq!(
            SettlementState::restore(&p, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = SettlementParts::default();
        bad.fallback_time = 7;
        assert_eq!(
            SettlementState::restore(&p, bad),
            Err(CoreError::InvariantViolated)
        );
    }
}
