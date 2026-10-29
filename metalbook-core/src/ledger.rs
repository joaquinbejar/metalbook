//! Vault accounting of one series.

use crate::{Contracts, CoreError, SeriesState, SeriesTerms, StrikeUsd, UsdcAtoms};

/// The USDC a series' vault must hold and the contracts it backs, through the series'
/// lifecycle: writers mint and close while `Active`, the series expires and settles, then
/// longs redeem and writers claim.
///
/// Every method computes all new values before changing anything, so a failed call leaves
/// the ledger unchanged. The accounting is exact:
/// - before settlement, `vault == open contracts × max_payout_per_contract`;
/// - after settlement, `vault == longs outstanding × payout + writer contracts unclaimed ×
///   residual`, so the vault reaches zero exactly when every long has redeemed and every
///   writer has claimed.
///
/// The on-chain vault may hold more than `vault` (anyone can transfer USDC to it); the program
/// checks `balance >= vault`, never equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesLedger {
    terms: SeriesTerms,
    state: SeriesState,
    /// Contracts written and not closed. Before settlement it equals the long supply.
    open: Contracts,
    /// Long tokens not yet redeemed. Equals `open` until settlement.
    longs_outstanding: Contracts,
    /// Written contracts whose writer has not claimed the residual. Equals `open` until
    /// settlement.
    writers_unclaimed: Contracts,
    /// Payout per long token; zero until settlement.
    payout: UsdcAtoms,
    /// Residual per written contract; zero until settlement.
    residual: UsdcAtoms,
    /// USDC the vault owes, in atoms.
    vault: UsdcAtoms,
}

impl SeriesLedger {
    /// An `Active` series with nothing written.
    #[must_use]
    pub const fn new(terms: SeriesTerms) -> Self {
        Self {
            terms,
            state: SeriesState::Active,
            open: Contracts::ZERO,
            longs_outstanding: Contracts::ZERO,
            writers_unclaimed: Contracts::ZERO,
            payout: UsdcAtoms::ZERO,
            residual: UsdcAtoms::ZERO,
            vault: UsdcAtoms::ZERO,
        }
    }

    /// Rebuilds a ledger from stored fields, which are untrusted: the parts must describe a
    /// state this type can reach, so the payout and residual must be zero before settlement
    /// and match `terms` at some price after it (`payout + residual == max_payout`), and the
    /// accounting identity must hold.
    ///
    /// # Errors
    ///
    /// `InvariantViolated` if the parts are inconsistent, `Overflow` if an amount does not fit.
    #[must_use = "the result carries a possible error"]
    pub fn restore(terms: SeriesTerms, parts: LedgerParts) -> Result<Self, CoreError> {
        let settled_amounts_ok = match parts.state {
            SeriesState::Active | SeriesState::Expired => {
                parts.payout == UsdcAtoms::ZERO && parts.residual == UsdcAtoms::ZERO
            }
            SeriesState::Settled => {
                parts.payout.checked_add(parts.residual)? == terms.max_payout_per_contract()?
            }
            SeriesState::Voided => {
                parts.payout == UsdcAtoms::ZERO
                    && parts.residual == terms.max_payout_per_contract()?
            }
        };
        if !settled_amounts_ok {
            return Err(CoreError::InvariantViolated);
        }
        let ledger = Self {
            terms,
            state: parts.state,
            open: parts.open,
            longs_outstanding: parts.longs_outstanding,
            writers_unclaimed: parts.writers_unclaimed,
            payout: parts.payout,
            residual: parts.residual,
            vault: parts.vault,
        };
        ledger.check_invariants()?;
        Ok(ledger)
    }

    /// The fields to store; [`SeriesLedger::restore`] reads them back.
    #[must_use]
    pub const fn parts(&self) -> LedgerParts {
        LedgerParts {
            state: self.state,
            open: self.open,
            longs_outstanding: self.longs_outstanding,
            writers_unclaimed: self.writers_unclaimed,
            payout: self.payout,
            residual: self.residual,
            vault: self.vault,
        }
    }

    /// The series terms.
    #[must_use]
    #[inline]
    pub const fn terms(&self) -> &SeriesTerms {
        &self.terms
    }

    /// The lifecycle state.
    #[must_use]
    #[inline]
    pub const fn state(&self) -> SeriesState {
        self.state
    }

    /// Contracts written and not closed.
    #[must_use]
    #[inline]
    pub const fn open_contracts(&self) -> Contracts {
        self.open
    }

    /// Long tokens not yet redeemed.
    #[must_use]
    #[inline]
    pub const fn longs_outstanding(&self) -> Contracts {
        self.longs_outstanding
    }

    /// Written contracts whose residual is not yet claimed.
    #[must_use]
    #[inline]
    pub const fn writers_unclaimed(&self) -> Contracts {
        self.writers_unclaimed
    }

    /// USDC the vault owes, in atoms.
    #[must_use]
    #[inline]
    pub const fn vault(&self) -> UsdcAtoms {
        self.vault
    }

    /// Payout per long token and residual per written contract, once `Settled` or `Voided`.
    #[must_use]
    pub const fn settlement_amounts(&self) -> Option<(UsdcAtoms, UsdcAtoms)> {
        match self.state {
            SeriesState::Settled | SeriesState::Voided => Some((self.payout, self.residual)),
            SeriesState::Active | SeriesState::Expired => None,
        }
    }

    /// A writer writes `n` contracts: returns the collateral to deposit, `n ×
    /// max_payout_per_contract`, and mints `n` long tokens.
    ///
    /// # Errors
    ///
    /// `SeriesNotActive`, `ZeroAmount`, or `Overflow`.
    #[must_use = "the deposit must be transferred"]
    pub fn mint(&mut self, n: Contracts) -> Result<UsdcAtoms, CoreError> {
        self.require(SeriesState::Active, CoreError::SeriesNotActive)?;
        require_nonzero(n)?;
        let deposit = self.terms.collateral(n)?;
        let vault = self.vault.checked_add(deposit)?;
        let open = self.open.checked_add(n)?;
        self.vault = vault;
        self.open = open;
        self.longs_outstanding = open;
        self.writers_unclaimed = open;
        Ok(deposit)
    }

    /// A writer burns `n` long tokens it holds before expiry: returns the collateral to
    /// withdraw, `n × max_payout_per_contract`. The caller checks that `n` does not exceed that
    /// writer's own open contracts; this checks the series total.
    ///
    /// # Errors
    ///
    /// `SeriesNotActive`, `ZeroAmount`, `InsufficientWritten`, `Overflow`, or
    /// `InvariantViolated` if the vault would go below zero.
    #[must_use = "the withdrawal must be transferred"]
    pub fn close(&mut self, n: Contracts) -> Result<UsdcAtoms, CoreError> {
        self.require(SeriesState::Active, CoreError::SeriesNotActive)?;
        require_nonzero(n)?;
        let open = self
            .open
            .checked_sub(n)
            .map_err(|_| CoreError::InsufficientWritten)?;
        let withdrawal = self.terms.collateral(n)?;
        let vault = self
            .vault
            .checked_sub(withdrawal)
            .map_err(|_| CoreError::InvariantViolated)?;
        self.vault = vault;
        self.open = open;
        self.longs_outstanding = open;
        self.writers_unclaimed = open;
        Ok(withdrawal)
    }

    /// `Active` to `Expired`: minting and closing stop. The caller decides when, from the
    /// clock.
    ///
    /// # Errors
    ///
    /// `SeriesNotActive`.
    pub fn expire(&mut self) -> Result<(), CoreError> {
        self.require(SeriesState::Active, CoreError::SeriesNotActive)?;
        self.state = SeriesState::Expired;
        Ok(())
    }

    /// `Expired` to `Settled` at `settlement`, in USDC atoms per ounce: fixes the payout per
    /// long token (rounded down) and the residual per written contract.
    ///
    /// # Errors
    ///
    /// `SeriesNotExpired`, `Overflow`, or `InvariantViolated` if the vault does not cover
    /// every long's payout and every writer's residual.
    pub fn settle(&mut self, settlement: StrikeUsd) -> Result<(), CoreError> {
        self.require(SeriesState::Expired, CoreError::SeriesNotExpired)?;
        let payout = self.terms.payout_per_contract(settlement)?;
        let residual = self.terms.writer_residual_per_contract(settlement)?;
        let owed = payout
            .checked_mul_contracts(self.open)?
            .checked_add(residual.checked_mul_contracts(self.open)?)?;
        if owed != self.vault {
            return Err(CoreError::InvariantViolated);
        }
        self.payout = payout;
        self.residual = residual;
        self.state = SeriesState::Settled;
        Ok(())
    }

    /// `Expired` to `Voided`: no settlement price exists (`docs/PLAN.md` 4.3, step 5). Longs
    /// receive nothing and each written contract returns its whole collateral to the writer,
    /// so the vault still empties exactly through the writer claims.
    ///
    /// # Errors
    ///
    /// `SeriesNotExpired`, `Overflow`, or `InvariantViolated` if the vault does not hold the
    /// collateral of every open contract.
    pub fn void(&mut self) -> Result<(), CoreError> {
        self.require(SeriesState::Expired, CoreError::SeriesNotExpired)?;
        let residual = self.terms.max_payout_per_contract()?;
        if residual.checked_mul_contracts(self.open)? != self.vault {
            return Err(CoreError::InvariantViolated);
        }
        self.payout = UsdcAtoms::ZERO;
        self.residual = residual;
        self.state = SeriesState::Voided;
        Ok(())
    }

    /// A holder burns `n` long tokens after settlement: returns `n × payout`. Also after a void,
    /// where the payout is zero, so that every long token can be burned and the series closed
    /// (`docs/PLAN.md` 4.6).
    ///
    /// # Errors
    ///
    /// `SeriesNotSettled`, `ZeroAmount`, `InsufficientLongTokens`, `Overflow`, or
    /// `InvariantViolated` if the vault would go below zero.
    #[must_use = "the payout must be transferred"]
    pub fn redeem(&mut self, n: Contracts) -> Result<UsdcAtoms, CoreError> {
        if !matches!(self.state, SeriesState::Settled | SeriesState::Voided) {
            return Err(CoreError::SeriesNotSettled);
        }
        require_nonzero(n)?;
        let longs = self
            .longs_outstanding
            .checked_sub(n)
            .map_err(|_| CoreError::InsufficientLongTokens)?;
        let amount = self.payout.checked_mul_contracts(n)?;
        let vault = self
            .vault
            .checked_sub(amount)
            .map_err(|_| CoreError::InvariantViolated)?;
        self.vault = vault;
        self.longs_outstanding = longs;
        Ok(amount)
    }

    /// A writer with `written` open contracts claims its residual after settlement (or its
    /// whole collateral after a void): returns
    /// `written × residual`. The caller enforces one claim per writer; this checks the series
    /// total.
    ///
    /// # Errors
    ///
    /// `SeriesNotSettled`, `ZeroAmount`, `InsufficientWritten`, `Overflow`, or
    /// `InvariantViolated` if the vault would go below zero.
    #[must_use = "the residual must be transferred"]
    pub fn claim_writer(&mut self, written: Contracts) -> Result<UsdcAtoms, CoreError> {
        if !matches!(self.state, SeriesState::Settled | SeriesState::Voided) {
            return Err(CoreError::SeriesNotSettled);
        }
        require_nonzero(written)?;
        let unclaimed = self
            .writers_unclaimed
            .checked_sub(written)
            .map_err(|_| CoreError::InsufficientWritten)?;
        let amount = self.residual.checked_mul_contracts(written)?;
        let vault = self
            .vault
            .checked_sub(amount)
            .map_err(|_| CoreError::InvariantViolated)?;
        self.vault = vault;
        self.writers_unclaimed = unclaimed;
        Ok(amount)
    }

    /// Checks the exact accounting identity for the current state (see the type docs).
    ///
    /// # Errors
    ///
    /// `InvariantViolated` if it does not hold, `Overflow` if an amount does not fit.
    pub fn check_invariants(&self) -> Result<(), CoreError> {
        let owed = match self.state {
            SeriesState::Active | SeriesState::Expired => {
                if self.longs_outstanding != self.open || self.writers_unclaimed != self.open {
                    return Err(CoreError::InvariantViolated);
                }
                self.terms.collateral(self.open)?
            }
            SeriesState::Settled | SeriesState::Voided => self
                .payout
                .checked_mul_contracts(self.longs_outstanding)?
                .checked_add(
                    self.residual
                        .checked_mul_contracts(self.writers_unclaimed)?,
                )?,
        };
        if owed == self.vault {
            Ok(())
        } else {
            Err(CoreError::InvariantViolated)
        }
    }

    fn require(&self, state: SeriesState, error: CoreError) -> Result<(), CoreError> {
        if self.state == state {
            Ok(())
        } else {
            Err(error)
        }
    }
}

/// The stored form of a [`SeriesLedger`], without its terms. A plain carrier: values read
/// from an account are untrusted until [`SeriesLedger::restore`] accepts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerParts {
    /// Lifecycle state.
    pub state: SeriesState,
    /// Contracts written and not closed.
    pub open: Contracts,
    /// Long tokens not yet redeemed.
    pub longs_outstanding: Contracts,
    /// Written contracts whose residual is not yet claimed.
    pub writers_unclaimed: Contracts,
    /// Payout per long token; zero until settlement.
    pub payout: UsdcAtoms,
    /// Residual per written contract; zero until settlement.
    pub residual: UsdcAtoms,
    /// USDC the vault owes, in atoms.
    pub vault: UsdcAtoms,
}

fn require_nonzero(n: Contracts) -> Result<(), CoreError> {
    if n.get() == 0 {
        Err(CoreError::ZeroAmount)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::SeriesLedger;
    use crate::{
        Contracts, CoreError, OptionKind, OuncesPerContract, SeriesKey, SeriesState, SeriesTerms,
        StrikeUsd, Underlying, UnixTimestamp, UsdcAtoms,
    };

    fn put_ledger() -> SeriesLedger {
        let key = SeriesKey::try_new(
            Underlying::Xau,
            OptionKind::Put,
            StrikeUsd::new(4_000_000_000),
            UnixTimestamp::new(1_800_000_000),
        )
        .unwrap();
        let terms =
            SeriesTerms::try_new(key, OuncesPerContract::ONE_OUNCE, StrikeUsd::new(0)).unwrap();
        SeriesLedger::new(terms)
    }

    #[test]
    fn test_ledger_full_lifecycle_empties_vault() {
        let mut l = put_ledger();
        assert_eq!(
            l.mint(Contracts::new(3)),
            Ok(UsdcAtoms::new(12_000_000_000))
        );
        assert_eq!(
            l.close(Contracts::new(1)),
            Ok(UsdcAtoms::new(4_000_000_000))
        );
        l.expire().unwrap();
        l.settle(StrikeUsd::new(3_900_000_000)).unwrap();
        assert_eq!(
            l.settlement_amounts(),
            Some((UsdcAtoms::new(100_000_000), UsdcAtoms::new(3_900_000_000)))
        );
        assert_eq!(l.redeem(Contracts::new(2)), Ok(UsdcAtoms::new(200_000_000)));
        assert_eq!(
            l.claim_writer(Contracts::new(2)),
            Ok(UsdcAtoms::new(7_800_000_000))
        );
        assert_eq!(l.vault(), UsdcAtoms::ZERO);
        l.check_invariants().unwrap();
    }

    #[test]
    fn test_ledger_state_gates() {
        let mut l = put_ledger();
        assert_eq!(
            l.redeem(Contracts::new(1)),
            Err(CoreError::SeriesNotSettled)
        );
        assert_eq!(
            l.claim_writer(Contracts::new(1)),
            Err(CoreError::SeriesNotSettled)
        );
        assert_eq!(
            l.settle(StrikeUsd::new(1)),
            Err(CoreError::SeriesNotExpired)
        );
        l.mint(Contracts::new(1)).unwrap();
        l.expire().unwrap();
        assert_eq!(l.state(), SeriesState::Expired);
        assert_eq!(l.mint(Contracts::new(1)), Err(CoreError::SeriesNotActive));
        assert_eq!(l.close(Contracts::new(1)), Err(CoreError::SeriesNotActive));
        assert_eq!(l.expire(), Err(CoreError::SeriesNotActive));
        l.settle(StrikeUsd::new(1)).unwrap();
        assert_eq!(
            l.settle(StrikeUsd::new(1)),
            Err(CoreError::SeriesNotExpired)
        );
    }

    #[test]
    fn test_ledger_rejects_zero_and_excess_amounts_without_change() {
        let mut l = put_ledger();
        assert_eq!(l.mint(Contracts::ZERO), Err(CoreError::ZeroAmount));
        l.mint(Contracts::new(2)).unwrap();
        let before = l;
        assert_eq!(
            l.close(Contracts::new(3)),
            Err(CoreError::InsufficientWritten)
        );
        assert_eq!(l, before);
        l.expire().unwrap();
        l.settle(StrikeUsd::new(0)).unwrap();
        let before = l;
        assert_eq!(
            l.redeem(Contracts::new(3)),
            Err(CoreError::InsufficientLongTokens)
        );
        assert_eq!(
            l.claim_writer(Contracts::new(3)),
            Err(CoreError::InsufficientWritten)
        );
        assert_eq!(l.redeem(Contracts::ZERO), Err(CoreError::ZeroAmount));
        assert_eq!(l, before);
    }

    #[test]
    fn test_ledger_restore_round_trips_and_rejects_inconsistent_parts() {
        let mut l = put_ledger();
        l.mint(Contracts::new(2)).unwrap();
        let t = *l.terms();
        assert_eq!(SeriesLedger::restore(t, l.parts()), Ok(l));
        let mut bad = l.parts();
        bad.vault = UsdcAtoms::new(1);
        assert_eq!(
            SeriesLedger::restore(t, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = l.parts();
        bad.payout = UsdcAtoms::new(1);
        assert_eq!(
            SeriesLedger::restore(t, bad),
            Err(CoreError::InvariantViolated)
        );
        let mut bad = l.parts();
        bad.longs_outstanding = Contracts::new(1);
        assert_eq!(
            SeriesLedger::restore(t, bad),
            Err(CoreError::InvariantViolated)
        );
        l.expire().unwrap();
        l.settle(StrikeUsd::new(3_000_000_000)).unwrap();
        assert_eq!(SeriesLedger::restore(t, l.parts()), Ok(l));
        let mut bad = l.parts();
        bad.residual = UsdcAtoms::ZERO;
        assert_eq!(
            SeriesLedger::restore(t, bad),
            Err(CoreError::InvariantViolated)
        );
    }

    #[test]
    fn test_ledger_void_returns_all_collateral_to_writers() {
        let mut l = put_ledger();
        l.mint(Contracts::new(3)).unwrap();
        assert_eq!(l.void(), Err(CoreError::SeriesNotExpired));
        l.expire().unwrap();
        l.void().unwrap();
        assert_eq!(l.state(), SeriesState::Voided);
        assert_eq!(
            l.settle(StrikeUsd::new(1)),
            Err(CoreError::SeriesNotExpired)
        );
        assert_eq!(SeriesLedger::restore(*l.terms(), l.parts()), Ok(l));
        // Long tokens can still be burned, for nothing, so the series can be closed.
        assert_eq!(l.redeem(Contracts::new(3)), Ok(UsdcAtoms::ZERO));
        assert_eq!(
            l.claim_writer(Contracts::new(3)),
            Ok(UsdcAtoms::new(12_000_000_000))
        );
        assert_eq!(l.vault(), UsdcAtoms::ZERO);
        l.check_invariants().unwrap();
    }

    #[test]
    fn test_ledger_mint_overflow_leaves_ledger_unchanged() {
        let mut l = put_ledger();
        l.mint(Contracts::new(1)).unwrap();
        let before = l;
        assert_eq!(l.mint(Contracts::new(u64::MAX)), Err(CoreError::Overflow));
        assert_eq!(l, before);
    }
}
