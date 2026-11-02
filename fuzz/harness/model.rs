//! Reference models for the fuzz targets, written from `docs/PLAN.md` (4.2, 4.3, 4.6) and the
//! payoff definitions, not from `metalbook-core`: the settlement candidates and cascade, and
//! the per-contract amounts of a series. Plain integers (`i128`, `u128`) and no shared code
//! with the crate, so a mistake on either side shows as a disagreement.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::disallowed_macros,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    dead_code,
    missing_docs
)]

/// Oracle terms of a series (the snapshot fields of `docs/PLAN.md` 4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    pub window_s: i64,
    pub slot_s: i64,
    pub tolerance_s: i64,
    pub min_samples: u32,
    pub k: usize,
    pub d: i64,
    pub lookback_s: i64,
    pub hard_s: i64,
    pub expo: i32,
}

impl Terms {
    pub fn slots(&self) -> u32 {
        (self.window_s / self.slot_s) as u32
    }
}

/// A print that passed Pyth's and the series' checks: price mantissa and times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Px {
    pub price: u64,
    pub publish: i64,
    pub prev: i64,
}

impl Px {
    /// `docs/PLAN.md` 3.3: a positive price, `prev_publish_time > 0`, and `prev <= publish`.
    pub fn valid(&self) -> bool {
        self.price > 0 && self.prev > 0 && self.prev <= self.publish
    }
}

/// What a print was stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Used {
    pub slot: Option<u8>,
    pub link: Option<u8>,
    pub fallback: bool,
}

/// The candidates of one series.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Candidates {
    pub slots: [Option<u64>; 32],
    pub chain: Vec<(i64, u64)>,
    pub fallback: Option<(i64, u64)>,
    pub window_changed: i64,
    pub next_changed: i64,
    pub fallback_changed: i64,
}

/// The cascade's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Wait(i64),
    /// Method as `SettleMethod` stores it (0 window, 1 next print, 2 partial chain, 3 partial
    /// window, 4 fallback), price in USDC atoms per ounce, number of prints averaged.
    Settle(u8, u64, u16),
    Void,
    /// The mean does not fit a `u64`.
    Overflow,
}

impl Candidates {
    pub fn filled(&self) -> u32 {
        self.slots.iter().filter(|s| s.is_some()).count() as u32
    }

    pub fn bitmap(&self) -> u32 {
        let mut b = 0u32;
        for (i, s) in self.slots.iter().enumerate() {
            if s.is_some() {
                b |= 1 << i;
            }
        }
        b
    }

    pub fn sample_sum(&self) -> u128 {
        self.slots.iter().flatten().map(|&p| u128::from(p)).sum()
    }

    /// Offers `x` at time `now`: stores it as every candidate it qualifies for, or returns
    /// `None` and changes nothing.
    pub fn offer(&mut self, t: &Terms, expiry: i64, x: Px, now: i64) -> Option<Used> {
        let publish = i128::from(x.publish);
        let prev = i128::from(x.prev);
        let expiry_w = i128::from(expiry);
        let start = expiry_w - i128::from(t.window_s);
        let mut used = Used::default();

        // 4.2: slot i is filled by the canonical print of t_i: published within the tolerance
        // after t_i, with the previous aggregate strictly before t_i.
        for i in 0..t.slots() {
            let t_i = start + i128::from(i) * i128::from(t.slot_s);
            if t_i <= publish
                && publish <= t_i + i128::from(t.tolerance_s)
                && prev < t_i
                && self.slots[i as usize].is_none()
            {
                used.slot = Some(i as u8);
                break;
            }
        }
        // 4.3 step 2: the chain of next prints after expiry, each strictly advancing.
        if self.chain.len() < t.k {
            let links = match self.chain.last() {
                None => prev <= expiry_w && expiry_w < publish,
                Some(&(last, _)) => prev == i128::from(last) && publish > i128::from(last),
            };
            if links {
                used.link = Some(self.chain.len() as u8);
            }
        }
        // 4.3 step 4: the latest first-of-its-second print in [expiry − X, expiry].
        let later = match self.fallback {
            None => true,
            Some((ft, _)) => publish > i128::from(ft),
        };
        if publish >= expiry_w - i128::from(t.lookback_s)
            && publish <= expiry_w
            && prev < publish
            && later
        {
            used.fallback = true;
        }

        if used == Used::default() {
            return None;
        }
        if let Some(i) = used.slot {
            self.slots[usize::from(i)] = Some(x.price);
            self.window_changed = now;
        }
        if used.link.is_some() {
            self.chain.push((x.publish, x.price));
            self.next_changed = now;
        }
        if used.fallback {
            self.fallback = Some((x.publish, x.price));
            self.fallback_changed = now;
        }
        Some(used)
    }

    /// The cascade of 4.3 at `now`.
    pub fn decide(&self, t: &Terms, expiry: i64, now: i64) -> Outcome {
        let now = i128::from(now);
        let end = i128::from(expiry);
        if now < end {
            return Outcome::Wait(expiry);
        }
        let d = i128::from(t.d);
        let hard = end + i128::from(t.hard_s);
        let filled = self.filled();
        let wait_or = |until: i128, o: Outcome| {
            if now >= until {
                o
            } else {
                Outcome::Wait(until as i64)
            }
        };
        let window = |method: u8| {
            let prices: Vec<u64> = self.slots.iter().flatten().copied().collect();
            mean(&prices, t.expo, method)
        };
        let chain = |method: u8| {
            let prices: Vec<u64> = self.chain.iter().map(|&(_, p)| p).collect();
            mean(&prices, t.expo, method)
        };
        if filled == t.slots() {
            return window(0);
        }
        if filled >= t.min_samples {
            return wait_or(end.max(i128::from(self.window_changed)) + d, window(0));
        }
        if self.chain.len() >= t.k {
            return wait_or(end + d, chain(1));
        }
        if now < hard {
            return Outcome::Wait(hard as i64);
        }
        if !self.chain.is_empty() {
            return wait_or(hard.max(i128::from(self.next_changed)) + d, chain(2));
        }
        if filled > 0 {
            return wait_or(hard.max(i128::from(self.window_changed)) + d, window(3));
        }
        if let Some((_, p)) = self.fallback {
            return wait_or(
                hard.max(i128::from(self.fallback_changed)) + d,
                mean(&[p], t.expo, 4),
            );
        }
        wait_or(hard + d, Outcome::Void)
    }
}

/// `price × 10^expo` in USDC atoms (6 decimals), floor and ceiling.
pub fn atoms_bounds(price: u64, expo: i32) -> (u128, u128) {
    let shift = 6 + expo;
    if shift >= 0 {
        let v = u128::from(price) * 10u128.pow(shift as u32);
        (v, v)
    } else {
        let s = 10u128.pow((-shift) as u32);
        let p = u128::from(price);
        (p / s, p.div_ceil(s))
    }
}

/// The mean of `prices` in USDC atoms, rounded to nearest with ties up (one rounding).
pub fn mean(prices: &[u64], expo: i32, method: u8) -> Outcome {
    let n = prices.len() as u128;
    let sum: u128 = prices.iter().map(|&p| u128::from(p)).sum();
    let shift = 6 + expo;
    let (num, den) = if shift >= 0 {
        (sum * 10u128.pow(shift as u32), n)
    } else {
        (sum, n * 10u128.pow((-shift) as u32))
    };
    let q = num / den;
    let r = num % den;
    let v = if 2 * r >= den { q + 1 } else { q };
    match u64::try_from(v) {
        Ok(v) => Outcome::Settle(method, v, prices.len() as u16),
        Err(_) => Outcome::Overflow,
    }
}

/// Per-contract amounts of a series (`docs/PLAN.md` 4.1, 4.6). `kind` 0 is a capped call, 1 a
/// put; amounts in USDC atoms; `mult` in milli-ounces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payoff {
    pub kind: u8,
    pub strike: u64,
    pub cap: u64,
    pub mult: u32,
}

impl Payoff {
    fn per_oz_max(&self) -> u128 {
        if self.kind == 1 {
            u128::from(self.strike)
        } else {
            u128::from(self.cap - self.strike)
        }
    }

    /// Collateral per contract: the exact maximum payout, rounded up (toward the protocol).
    pub fn collateral(&self) -> u64 {
        (self.per_oz_max() * u128::from(self.mult)).div_ceil(1_000) as u64
    }

    /// Payout per long token at settlement price `s`, rounded down (toward the protocol).
    pub fn payout(&self, s: u64) -> u64 {
        let s = u128::from(s);
        let k = u128::from(self.strike);
        let per_oz = if self.kind == 1 {
            k.saturating_sub(s)
        } else {
            s.saturating_sub(k).min(self.per_oz_max())
        };
        (per_oz * u128::from(self.mult) / 1_000) as u64
    }

    /// What a writer gets back per contract: the collateral minus the payout (all the dust).
    pub fn residual(&self, s: u64) -> u64 {
        self.collateral() - self.payout(s)
    }
}
