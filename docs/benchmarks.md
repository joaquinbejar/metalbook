# Compute units

Measured by `make bench` (`metalbook-tests/tests/cu.rs`) in LiteSVM, sBPF v3, including the
CPIs each instruction makes (system, token, associated token). Deterministic: two fresh
runs must agree exactly.

| Instruction | Case | CU |
| --- | --- | ---: |
| `init_config` | creates the config | 6773 |
| `set_config` | same admin | 965 |
| `set_calendar` | creates the calendar, 2 expiries | 6581 |
| `set_calendar` | replaces the list, 64 expiries | 1310 |
| `create_series` | put | 29595 |
| `create_series` | capped call | 34213 |
| `mint` | first mint, creates the position | 9226 |
| `mint` | existing position | 4809 |
| `close` | partial | 4774 |
| `close` | to zero | 4774 |
| `sample_settlement` | first print: slot 0 and fallback, expires the series | 2512 |
| `sample_settlement` | window slot and fallback | 2517 |
| `sample_settlement` | next-print link | 2401 |
| `settle` | full window (30 slots) | 3199 |
| `redeem` | 1 long token, in the money | 4061 |
| `claim_writer` | residual of 1 contract | 3157 |
| `settle` | void | 2267 |
