# Compute units

Measured by `make bench` (`metalbook-tests/tests/cu.rs`) in LiteSVM, sBPF v3, including the
CPIs each instruction makes (system, token, associated token). Deterministic: two fresh
runs must agree exactly.

| Instruction | Case | CU |
| --- | --- | ---: |
| `init_config` | creates the config | 6749 |
| `set_config` | same admin | 942 |
| `set_calendar` | creates the calendar, 2 expiries | 6582 |
| `set_calendar` | replaces the list, 64 expiries | 1311 |
| `create_series` | put | 29600 |
| `create_series` | capped call | 34218 |
| `mint` | first mint, creates the position | 9227 |
| `mint` | existing position | 4810 |
| `close` | partial | 4776 |
| `close` | to zero | 4776 |
| `sample_settlement` | first print: slot 0 and fallback, expires the series | 2513 |
| `sample_settlement` | window slot and fallback | 2518 |
| `sample_settlement` | next-print link | 2402 |
| `settle` | full window (30 slots) | 3200 |
| `redeem` | 1 long token, in the money | 4064 |
| `claim_writer` | residual of 1 contract | 3157 |
| `close_position` | claimed position, rent to the writer | 356 |
| `close_series` | vault and series closed, rent to the payer | 2874 |
| `settle` | void | 2268 |
