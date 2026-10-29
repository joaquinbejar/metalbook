# Compute units

Measured by `make bench` (`metalbook-tests/tests/cu.rs`) in LiteSVM, sBPF v3, including the
CPIs each instruction makes (system, token, associated token). Deterministic: two fresh
runs must agree exactly.

| Instruction | Case | CU |
| --- | --- | ---: |
| `init_config` | creates the config | 6310 |
| `set_config` | same admin | 505 |
| `set_calendar` | creates the calendar, 2 expiries | 6581 |
| `set_calendar` | replaces the list, 64 expiries | 1310 |
| `create_series` | put | 29362 |
| `create_series` | capped call | 33980 |
| `mint` | first mint, creates the position | 9197 |
| `mint` | existing position | 4780 |
| `close` | partial | 4745 |
| `close` | to zero | 4745 |
