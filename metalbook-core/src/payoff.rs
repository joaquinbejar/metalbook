//! Payoff and collateral per contract, in USDC atoms.
//!
//! Every formula works per ounce in USDC atoms, then scales by the multiplier in
//! milli-ounces, which is the only lossy step. Its rounding direction is fixed by who
//! receives the amount: collateral (the maximum payout) is rounded up and the settlement
//! payout is rounded down, both toward the protocol.

use crate::{
    Contracts, CoreError, OptionKind, OuncesPerContract, SeriesTerms, StrikeUsd, UsdcAtoms,
};

/// Milli-ounces in one troy ounce: the divisor of [`OuncesPerContract`].
pub const MILLI_OUNCES_PER_OUNCE: u32 = 1_000;

/// Direction of the single lossy step, the scaling by the multiplier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rounding {
    /// Toward zero.
    Down,
    /// Away from zero.
    Up,
}

/// `per_ounce × multiplier / 1000` in USDC atoms, rounded in direction `rounding`.
///
/// The product of a `u64` and a `u32` always fits a `u128`, so only the final narrowing can
/// fail.
fn scale_per_contract(
    per_ounce: u64,
    multiplier: OuncesPerContract,
    rounding: Rounding,
) -> Result<UsdcAtoms, CoreError> {
    let product = u128::from(per_ounce)
        .checked_mul(u128::from(multiplier.get()))
        .ok_or(CoreError::Overflow)?;
    let divisor = u128::from(MILLI_OUNCES_PER_OUNCE);
    let quotient = product.checked_div(divisor).ok_or(CoreError::Overflow)?;
    let remainder = product.checked_rem(divisor).ok_or(CoreError::Overflow)?;
    let rounded = match rounding {
        Rounding::Up if remainder != 0 => quotient.checked_add(1).ok_or(CoreError::Overflow)?,
        Rounding::Up | Rounding::Down => quotient,
    };
    u64::try_from(rounded)
        .map(UsdcAtoms::new)
        .map_err(|_| CoreError::Overflow)
}

impl SeriesTerms {
    /// The largest payoff per ounce, in USDC atoms: `K` for a put, `cap − K` for a call.
    fn max_payoff_per_ounce(&self) -> Result<u64, CoreError> {
        let strike = self.key().strike().get();
        match self.key().kind() {
            OptionKind::Put => Ok(strike),
            OptionKind::Call => self
                .cap()
                .get()
                .checked_sub(strike)
                .ok_or(CoreError::InvalidCap),
        }
    }

    /// The payoff per ounce at settlement price `s`, in USDC atoms, exact:
    /// `max(K − S, 0)` for a put, `min(max(S − K, 0), cap − K)` for a call.
    fn payoff_per_ounce(&self, settlement: StrikeUsd) -> Result<u64, CoreError> {
        let strike = self.key().strike().get();
        let s = settlement.get();
        // Out of the money (`None`) pays zero; written as a match, not `saturating_sub`, which
        // the rules ban on financial values.
        let in_the_money = match self.key().kind() {
            OptionKind::Put => strike.checked_sub(s),
            OptionKind::Call => s.checked_sub(strike),
        };
        let gain = match in_the_money {
            Some(gain) => gain,
            None => return Ok(0),
        };
        match self.key().kind() {
            OptionKind::Put => Ok(gain),
            OptionKind::Call => Ok(gain.min(self.max_payoff_per_ounce()?)),
        }
    }

    /// The maximum payout of one contract in USDC atoms, which is also the collateral a
    /// writer deposits per contract: `K × mult` for a put, `(cap − K) × mult` for a call.
    ///
    /// Rounded up, toward the protocol: the vault holds at least the exact maximum payout.
    ///
    /// # Errors
    ///
    /// `Overflow` if the amount does not fit a `u64`; never for terms built by
    /// [`SeriesTerms::try_new`], which checks it.
    #[must_use = "the result carries a possible error"]
    pub fn max_payout_per_contract(&self) -> Result<UsdcAtoms, CoreError> {
        scale_per_contract(
            self.max_payoff_per_ounce()?,
            self.multiplier(),
            Rounding::Up,
        )
    }

    /// The collateral for `n` contracts: `n × max_payout_per_contract`, exact.
    ///
    /// # Errors
    ///
    /// `Overflow` if the amount does not fit a `u64`.
    #[must_use = "the result carries a possible error"]
    pub fn collateral(&self, n: Contracts) -> Result<UsdcAtoms, CoreError> {
        self.max_payout_per_contract()?.checked_mul_contracts(n)
    }

    /// What one long token receives at settlement price `settlement`.
    ///
    /// Rounded down, toward the protocol (against the long). Never above
    /// [`SeriesTerms::max_payout_per_contract`]: the exact payoff is at most the exact
    /// maximum, and rounding one down and the other up keeps the order.
    ///
    /// # Errors
    ///
    /// `Overflow` if the amount does not fit a `u64`; never for terms built by
    /// [`SeriesTerms::try_new`].
    #[must_use = "the result carries a possible error"]
    pub fn payout_per_contract(&self, settlement: StrikeUsd) -> Result<UsdcAtoms, CoreError> {
        scale_per_contract(
            self.payoff_per_ounce(settlement)?,
            self.multiplier(),
            Rounding::Down,
        )
    }

    /// What a writer receives back per contract written at settlement price `settlement`:
    /// `max_payout_per_contract − payout_per_contract`. It includes the rounding dust of both
    /// amounts, so the dust's owner is the writer and the vault empties exactly.
    ///
    /// # Errors
    ///
    /// - `Overflow` as for the two amounts it subtracts.
    /// - `InvariantViolated` if the payout exceeded the maximum payout (never expected).
    #[must_use = "the result carries a possible error"]
    pub fn writer_residual_per_contract(
        &self,
        settlement: StrikeUsd,
    ) -> Result<UsdcAtoms, CoreError> {
        self.max_payout_per_contract()?
            .checked_sub(self.payout_per_contract(settlement)?)
            .map_err(|_| CoreError::InvariantViolated)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]
mod tests {
    use crate::{
        Contracts, CoreError, OptionKind, OuncesPerContract, SeriesKey, SeriesTerms, StrikeUsd,
        Underlying, UnixTimestamp, UsdcAtoms,
    };

    /// 1 USDC in atoms.
    const USD: u64 = 1_000_000;

    fn terms(kind: OptionKind, strike_usd: u64, cap_usd: u64, milli_oz: u32) -> SeriesTerms {
        let key = SeriesKey::try_new(
            Underlying::Xau,
            kind,
            StrikeUsd::new(strike_usd * USD),
            UnixTimestamp::new(1_800_000_000),
        )
        .unwrap();
        SeriesTerms::try_new(
            key,
            OuncesPerContract::try_new(milli_oz).unwrap(),
            StrikeUsd::new(cap_usd * USD),
        )
        .unwrap()
    }

    fn px(usd: u64) -> StrikeUsd {
        StrikeUsd::new(usd * USD)
    }

    #[test]
    fn test_put_payout_itm_atm_otm() {
        let t = terms(OptionKind::Put, 4_000, 0, 1_000);
        assert_eq!(t.max_payout_per_contract(), Ok(UsdcAtoms::new(4_000 * USD)));
        assert_eq!(
            t.payout_per_contract(px(3_900)),
            Ok(UsdcAtoms::new(100 * USD))
        );
        assert_eq!(t.payout_per_contract(px(4_000)), Ok(UsdcAtoms::ZERO));
        assert_eq!(t.payout_per_contract(px(4_100)), Ok(UsdcAtoms::ZERO));
        assert_eq!(
            t.payout_per_contract(StrikeUsd::new(0)),
            Ok(UsdcAtoms::new(4_000 * USD))
        );
    }

    #[test]
    fn test_capped_call_payout_is_bounded_by_cap() {
        let t = terms(OptionKind::Call, 4_000, 4_400, 1_000);
        assert_eq!(t.max_payout_per_contract(), Ok(UsdcAtoms::new(400 * USD)));
        assert_eq!(t.payout_per_contract(px(3_900)), Ok(UsdcAtoms::ZERO));
        assert_eq!(t.payout_per_contract(px(4_000)), Ok(UsdcAtoms::ZERO));
        assert_eq!(
            t.payout_per_contract(px(4_150)),
            Ok(UsdcAtoms::new(150 * USD))
        );
        assert_eq!(
            t.payout_per_contract(px(4_400)),
            Ok(UsdcAtoms::new(400 * USD))
        );
        assert_eq!(
            t.payout_per_contract(px(9_000)),
            Ok(UsdcAtoms::new(400 * USD))
        );
        assert_eq!(
            t.payout_per_contract(StrikeUsd::new(u64::MAX)),
            Ok(UsdcAtoms::new(400 * USD))
        );
    }

    #[test]
    fn test_silver_multiplier_scales_payout() {
        // XAG, 50 oz per contract, K = 48 USD, cap = 60 USD.
        let t = terms(OptionKind::Call, 48, 60, 50_000);
        assert_eq!(t.max_payout_per_contract(), Ok(UsdcAtoms::new(600 * USD)));
        assert_eq!(t.payout_per_contract(px(50)), Ok(UsdcAtoms::new(100 * USD)));
        assert_eq!(
            t.collateral(Contracts::new(3)),
            Ok(UsdcAtoms::new(1_800 * USD))
        );
    }

    #[test]
    fn test_rounding_collateral_up_payout_down_residual_holds_dust() {
        // 1 milli-ounce per contract: every amount is divided by 1000.
        let key = SeriesKey::try_new(
            Underlying::Xau,
            OptionKind::Put,
            StrikeUsd::new(1_001),
            UnixTimestamp::new(0),
        )
        .unwrap();
        let t = SeriesTerms::try_new(
            key,
            OuncesPerContract::try_new(1).unwrap(),
            StrikeUsd::new(0),
        )
        .unwrap();
        // Exact maximum 1.001 atoms: collateral 2, payout at S = 0 is 1, residual 1.
        assert_eq!(t.max_payout_per_contract(), Ok(UsdcAtoms::new(2)));
        assert_eq!(
            t.payout_per_contract(StrikeUsd::new(0)),
            Ok(UsdcAtoms::new(1))
        );
        assert_eq!(
            t.writer_residual_per_contract(StrikeUsd::new(0)),
            Ok(UsdcAtoms::new(1))
        );
    }

    #[test]
    fn test_collateral_overflow_is_reported() {
        let t = terms(OptionKind::Put, 4_000, 0, 1_000);
        assert_eq!(
            t.collateral(Contracts::new(u64::MAX)),
            Err(CoreError::Overflow)
        );
    }
}
