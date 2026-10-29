# MetalBook

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Cash-settled options on gold and silver on Solana, settled in USDC.

European options on XAU and XAG priced with optfix, quoted through QuoteBook, collateralized in program vaults and settled from Pyth price feeds at expiry.

**Status:** milestone 4 (domain math and settlement rules in `metalbook-core`; series, mint, close, Pyth settlement, redeem and writer claims in `metalbook-program`). See `docs/PLAN.md`.
