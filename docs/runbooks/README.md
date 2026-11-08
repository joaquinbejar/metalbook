# Runbooks

One page per situation, each with its commands, exact arguments and expected output.
`metalbook-watch` names the page in every alert it sends.

| Page | When |
| --- | --- |
| [keeper-restart.md](keeper-restart.md) | Stopping, restarting or moving the keeper; a silent keeper; overdue slots |
| [hermes-outage.md](hermes-outage.md) | Hermes errors, 429 storms, a rejected API key |
| [rpc-outage.md](rpc-outage.md) | The JSON-RPC endpoint fails or rate-limits |
| [low-balance.md](low-balance.md) | A payer under the 0.5 SOL alarm |
| [orphaned-vaas.md](orphaned-vaas.md) | Leftover encoded VAAs holding a payer's rent |
| [stuck-settlement.md](stuck-settlement.md) | A series past its window and not settled; error 141 |
| [void.md](void.md) | A series nothing can price: void, refunds, close |
| [upgrade-rollback.md](upgrade-rollback.md) | Deploying a new program binary, or the previous one |
| [key-compromise.md](key-compromise.md) | Any role's key, the Hermes key or the maker's key is exposed |
| [series-roll.md](series-roll.md) | The weekly roller: `NoNextSeries`, `CalendarLow`, a failed or busy run, extending the calendar |

Keys and roles are in `docs/keys.md`; what is in scope for an audit is in
`docs/audit-scope.md`.
