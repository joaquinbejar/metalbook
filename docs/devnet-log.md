# Devnet deployment log

The first devnet deployment (milestone 7), run on 2026-10-30 from the runbook in
[`devnet.md`](devnet.md). Every address and signature can be checked on devnet
(`https://explorer.solana.com/tx/<SIGNATURE>?cluster=devnet`).

## Program

| Item | Value |
| --- | --- |
| Program id | `FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg` |
| ProgramData | `DSptVxC9DvwxqEK3qY4hTUuHb8K6e6wEbWi9r18XMRVF` |
| Upgrade authority, admin and payer | `77ieYLKspRnGzAt1AK6ZVsttnTY6T7EY2k7CpTZeCPPV` until 2026-11-07; one key per role since (Key separation below, `docs/keys.md`) |
| Commit | `93f175a46e8e9b9a11bae2bf7fb8cde497a49fa8` |
| `metalbook_program.so` sha256 | `4b58cb509d38805a1e910ebbae25ebe292186eea45d0965dcaffcc5a4c954127` (90,888 bytes) |
| Deployed in slot | 516765484 |
| USDC mint | `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU` (Circle devnet USDC, 6 decimals) |
| Config | `CgJYkT8Q3KnsAMj8gPJpwUfabiu66ezLQ4NjxVsjMZwx`, parameters `metalbook-cli/devnet-params.json` |
| XAU calendar | `2p6wAvNefHwGsTDkMMLU5JpSE2iPZyekEfBLXcstFpwh` |
| XAG calendar | `Cy6qBFFPRymJNmQwDkhaNoggf3vk2WyDCBbD9e5K3FUY` |
| Maker | `7tVPWX3zmxYCZ5ioTSX4yzdNq8o3Xr1hMSv2kz12cACQ` (2 SOL from the payer) |
| Taker | `46devdqDGUzbaHZSYDhiWmEKX3VVGHHLzH8JzaTEbmQk` (2 SOL from the payer) |

The program id, maker and taker keypairs live outside the repository
(`~/.config/solana/metalbook-devnet-{program,maker,taker}.json` on the deploying machine).

## Transactions

| Step | Signature |
| --- | --- |
| `solana program deploy` | `3hqiegT28PbEDv9ky8ThP6uBtVgyVqDp5VShiawbuMnusm2BXeDggfU98pkuumBsrrrSMnBgXiChDf6SPGLqqnod` |
| `init_config` | `3dWFtHCVHUwB6pP9nMx1XMujMvbHgXG6ybeidaqML5npGLEUUtKuJAxJoVUBoR9sahes3tsfHnax6axYjFjLD7iQ` |
| `set_calendar` XAU | `4gKUpujzPtbEMBv5mpME7buHBSPof4TyAXFtjQJnYCFkp6NNzTXcnJ1vb4CcTVMH4EnmmWULXFCCjLKQaQWtkKvr` |
| `set_calendar` XAG | `58zrEshre5QGVHqyErBh7Fvuuwc6yP1Y2Djxqg4ntNwGHpnwbK8qRevgjA13PU4Tnz5tMt4vE6CDd4o7ookdnf5g` |
| Fund maker (2 SOL) | `48vuATAEE6gFuLd151eASEE2csRvBA32EkdVVWW53rQK59DCRU8xueHep1tcKLtMSWcKXfznaLRY4BVE3be5wSd8` |
| Fund taker (2 SOL) | `2HAjjnzDRk75JMiJgSzF5yD1S7Ntwf7V2ZBU6Vx2nZJZCSk42kWKLbjr4LTM59ABqRMZHScWNhi1WmHnpXEzQBFq` |

## Calendar

Weekly Fridays at 16:00 America/New_York, for both metals (Unix seconds):

| Expiry (New York) | UTC | Unix |
| --- | --- | --- |
| Fri 2026-10-30 16:00 EDT | 20:00Z | 1793390400 |
| Fri 2026-11-06 16:00 EST | 21:00Z | 1793998800 |
| Fri 2026-11-13 16:00 EST | 21:00Z | 1794603600 |
| Fri 2026-11-20 16:00 EST | 21:00Z | 1795208400 |
| Fri 2026-12-04 16:00 EST | 21:00Z | 1796418000 |
| Fri 2026-12-11 16:00 EST | 21:00Z | 1797022800 |
| Fri 2026-12-18 16:00 EST | 21:00Z | 1797627600 |

Left out: Friday 2026-11-27 (CME metals close early the day after Thanksgiving, before 16:00)
and Friday 2026-12-25 (Christmas, closed). `set_calendar` replaces the list, so later weeks
are added the same way.

## Series

Strikes at the money ±5% on the approved steps ($50 XAU, $1 XAG) around the spot at listing
(XAU 3,968.15 from the mainnet Pyth push account `2uPQGpm8X4ZkxMHxrAW1QuhXcse1AHEgPih6Xp9NuEWW`,
48 s old; XAG 50.357 from a public spot quote, because the XAG push accounts are stale and
Hermes requires a key, see below). Each strike has a put and a capped call with the cap 10%
above the strike (`cap_bps` 1000). Multipliers from the config: XAU 1 oz, XAG 50 oz. Listed
for the first two expiries; later expiries are listed weekly around the then spot.

| Expiry | Metal | Kind | Strike | Series | `create_series` signature |
| --- | --- | --- | ---: | --- | --- |
| 1793390400 | XAU | put | 3800 | `2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5` | `415veQYsijvk6sSHRiyMDCAUvwMvtnkWRFnpvFUiUF42URCyT8yYNuzk7BAg5oXiJyx4WTHUEMN6xa8MyNon8FU5` |
| 1793390400 | XAU | call (cap 4180) | 3800 | `BvhmabWKVnDWacexfQw58U1uGbU3HLDV4AfHKquuaBbi` | `5o5YRKA8BkadRKj9JruDrSLgPyYP5cAbc8TxARdyxPq7PLY8hrkfYxWc33rb4wzUJDTABMfDJ9iyZCfnWk22r6fb` |
| 1793390400 | XAU | put | 3850 | `ANtq1DXY6rg92dag8sLHLzm8dMG5t2Pn6CZzZw145oon` | `3oN8q89oozEWBZ5skA24fSaFMWPTRBsh7PnHvsUQ5tSU9xzJRuBYnzezfbD35X438NsX7ii9kabzHtuUY1xsqyS1` |
| 1793390400 | XAU | call (cap 4235) | 3850 | `Dr5DN1cyYkNq4Xe9cKEud7cXDPjk57ERhhzvUQdBXHA` | `2sucno6Je2uGic96QbUDww4ckp87fUk9HjhwuaFG35xPrCtdH5vRi9t2TgW5Aov9BDDtmC8GLVPr3TMa3QsLPdQe` |
| 1793390400 | XAU | put | 3900 | `5vvy5enzqBm5zfZnUFnDs8K5AG8JEEwNbspvV2CDusbN` | `5Gcy23nVqBy6chw2yRksxnb7XWTbwgrdcwBHB7zoBPMuatZMU3Dtzc8Wm3ybX59A2w7689UkUUm2bGaPoHrMDCHQ` |
| 1793390400 | XAU | call (cap 4290) | 3900 | `4W67aaLu1gUYwBszn9Zubt1hAq2wy4SL5N7XRUczF9St` | `TybMnigp5TK5QAZXM1GQVZC3xaBfZ47o6n5VBvJpJhP4CJC2eZVwejAaqzt4mrfATqWUMmLYu6mFU7qzvQDEGJg` |
| 1793390400 | XAU | put | 3950 | `AqnB9PpgUAhW4tmqzt2Srzsgdfm2BzU6m2jbbfYYpBi4` | `312DmXdvUhqP9r7zMPyBRwdQNKi9R1s534HAqrqnbgDkrddPoy1FJCt7gxykX99VAWPRSS2R9SDRUHoxXPJ49ifP` |
| 1793390400 | XAU | call (cap 4345) | 3950 | `JvKCAALDRAxd77sCLw1as17CQDXbvsg2u4ydbtGxQAi` | `5TesANVUWTNB4YdtBc8g4bEpZni3qBo4DFSj2TvdHqnR6NiA4EW1cG8t8rtFYaFM1Uoe9tsXpLW8eciY2y6vwzxu` |
| 1793390400 | XAU | put | 4000 | `4u74R8Zn88TTxFHaDBVifQKSHgyJbsGCo9ndhG4raXhv` | `41dbzKcUypEfp5JetysBbXrzJhDm2vfPWpLey84yh9cAFLgeqg3iPWSYaqcMDPTqi7TwQE9tWPUgJZ8bmhjZKGNL` |
| 1793390400 | XAU | call (cap 4400) | 4000 | `92xnofbYSfeSqvoxrdNoPfeU9kpfr7FDu8JeijgasQiw` | `qaBLjAHaTEfPVqiHN6HKVWGkKJHF2LAahyeXskTo2Z2sHBPHgLyuuvPgybByojjoAsnAgU5Wk4Y6LK9oGs42W85` |
| 1793390400 | XAU | put | 4050 | `8Lv5WwKejsFCrawBAvVvUgEdsSmCBaARevXgLzvSUCAU` | `4RKKNBeYNfMAWDraAxrDxBzb4xNL9pmqrzVf7twJorMnx4Uys2jdvQsWgdGtKYR621eZom6wRwLWf5KmNyXPxLcx` |
| 1793390400 | XAU | call (cap 4455) | 4050 | `6ZbdqcGTddC3SoTtggHEWWyDj4XJsFGwU58GVZmktmyM` | `2NzXxQhtasGJYbfQB7xvtop3jyi4jrPsnE3HUNbDXB5gDYxKLfMRV2gXsBswWJupeCT5V1gkJJobXnj2KKTmFKkL` |
| 1793390400 | XAU | put | 4100 | `Dw3skzBfb9S3kBrRnAvSkvGKoZcRbFy178yZ93WSiL1y` | `3SpoRNpFpPQgrdCHhYPDpLyJXss3HkmqpDHWy1Hta5rSuUx4TWzWisT78gfUg9742WXizM9qiWAESFmjZfZkeUhx` |
| 1793390400 | XAU | call (cap 4510) | 4100 | `6qg3wruL2TocFw9f4Q3CXA8mmWx6GKsGeMTUphPJgF5H` | `5XwZPxFaSq6dMSnQdCvhSB3dPpwm17ravqjNZVHTFNKCSYspwXpbgzvoS43mhy8dG5AMkUYZJUh3ATEhA2Ww99e2` |
| 1793390400 | XAU | put | 4150 | `EfyCdzYUmTY8ie6XtbHpQAgzA1bBZdhC47wK6Ez6JyRU` | `5fwDRgDRhDuZ1mTkbB3oRBAoPGB7R3aHGiCxZbWLdZ3WkQtBx4FanTHn4UqMkRCzWEP7oTNyT2VjgeXFyAVxpJr3` |
| 1793390400 | XAU | call (cap 4565) | 4150 | `3hhKKRXq8bJne9aPH2P5vtDjnaY1neJuTRpVhzDy53Jq` | `57kfuESzj6A1DBevBDKVrBtMe7EDvCYUQmNdzCkWXWXsH5xgVDuCWcTZWfPNuVoo1QBWLuK4MsL2BUSbici7h4R` |
| 1793390400 | XAG | put | 48 | `7WStW2SFVLeeTYrBCMszTZLgMkgDH2YJRABEHR7iiTao` | `59Pre636uoFE45r35JZvXXqKXHMYNcygFF2G9x9c5csGyoA5uEfXSBBs3J21LgVsybnutKURBD8d5ioEmY5pcFBY` |
| 1793390400 | XAG | call (cap 52.8) | 48 | `7EUNY3M7579s2M4UnmDS7QscJK61a5rrpPN9WQNn5wVw` | `F6QkPzUySuv3UptDD52Z4g3C9q4V1mAsRAroFz1esQ424sgptbSxwxYQHMr25JyCPzBZk9Nq8GRXAhGyNXWBP7w` |
| 1793390400 | XAG | put | 49 | `7gDF4kJGSQMzHcqoWB3FVqEvV8AuHH53JC5r1jBRN377` | `2hAvSffzpcDA1zMF6gYD4xnaWp53GsQCAaP3tL7qnzPrQnCf64DiJZdXD3b2HzGK1TRTUCz9tJK34bDpKyTMTk2w` |
| 1793390400 | XAG | call (cap 53.9) | 49 | `HbovhXFHPTJDj3EkCiEyzaSKsXCocxH5ot5djtcGERLW` | `2wxrg7oeKhsiN5mRyJm3ycP1poonhwrjtofadxfKU71Tbka7LwBihnLQExiV56NzRRPvYPFyz7hibRGVT23uEhxp` |
| 1793390400 | XAG | put | 50 | `4F2hdKE6gJ7TvMBq14pWgsBfKb3VxhTNA96CbYkqsWHn` | `5BjUYejeJU777katoKNcBVwuFEZqQE1fG87ypDX4o5CcoaV4bqdbudtTgig11VTxFapGpRDYLFtpaPqZAbngJxmR` |
| 1793390400 | XAG | call (cap 55) | 50 | `84uyQjzN6MkRi7XCx8q1xEqqhQFYavRAt119jMXXn4BY` | `5zF72pUDKHsNN6npfMhtFqi1usiDDSFiWTKnyd2Seg74QcftbrKp2R9Hjmserod7VV1icfiu3kLFR3HgKfzZmc5o` |
| 1793390400 | XAG | put | 51 | `GZz1xtQKoUjAj7v3x63PBgCDv1UexqdmK25HuyDqzmWJ` | `4H4Qt5sN6x7uj9XJp5LkDoLqK3vuuXmi8M2xmDnzUoYsBn96bhzPYHbBbfHz6L8yhUbrsDT46MdPBtJ6tzJQgH9M` |
| 1793390400 | XAG | call (cap 56.1) | 51 | `Ew94BcV7MYXiNx77yooBXteEe4XsxykZodU7Xg5MkpPD` | `3Ts3fsVm2kY1HuAo3zYqNVUCh2veJGfr6f8kuJsxtdswiYf6KjdzjFDRmgRr4w96SsPybKsiUB7rXj78oDSpPmSv` |
| 1793390400 | XAG | put | 52 | `6SYM398CRzH6j2u9gikyGCrSeRtdR3QK3pCCkrMFik31` | `ukXmT3Hbnhvznr8apvo4ErZQPpxVLsQRZaxa7JM1c2nA3EfJ8Zjj3ujvLMExtpLCDZqNwzPk7R3fR4MghgrvTPf` |
| 1793390400 | XAG | call (cap 57.2) | 52 | `Df5oVmx94PJM5BQEmk2QeWr7DEgaHaZ8TpBwzyDfaMX` | `2KftaL7yzqWSEM6efernv7Nit7wBreQBEvTk3vQRUyCULNPKwPFp37xmRUBhCkGVY28x9PCdEsQkQcD7zVJUWbgC` |
| 1793998800 | XAU | put | 3800 | `JAUr2AcDZq1YH11fBjmKwu2FQVKm6Jh1h2V6r4exy5iS` | `4fgrvQvXRyTDDZVTg1RMYRxpuFGpnhCJZjqkaXbnZkHFArVR76h4ukZUpHJTDbBCZ5nMzAnAowesZ3BGik9shYEY` |
| 1793998800 | XAU | call (cap 4180) | 3800 | `3F7YhT2Qfx5ASHfYNyHyuefZqbD4YvTE47caa9Fz6uWr` | `4vNuUSpKUmBZTJxgAG97YWZKRrMyir6ogHsVFoPddQt2SYvnmVTjGpFjCTCQ3L1fGmHSV5Tn89cVPMHmrZCDfgGM` |
| 1793998800 | XAU | put | 3850 | `8qTijtsquv5DEf3UJzwCYM4xnvtvJBKBtR2X3UVjr6QY` | `5iWumB6HrhWGXqpTgsi7wrJuQE3vgWXNmRX4VSxwn5VNnV9h9yK6TfQ8jLqQr3L6eAXCW2jjybfJpwqUWcBMWaPR` |
| 1793998800 | XAU | call (cap 4235) | 3850 | `F4katJN6EAMXoHmUTn3jABafiRk8XgxoY5jQPjfhBTVZ` | `3CrcVUx6qmPXtkENhEWgSC522jQMWHkUzZJixcNzCtzDcNyeQDyseEahAW8j2X5jyUTsjSjmVnKa9FEYXgXPEsqr` |
| 1793998800 | XAU | put | 3900 | `GCbeoaTa1yF4azv8hakR8jQ5CGVwmm6HmfHJ15SBRxbt` | `5jDwHfgqm7QEoG7wMVNEBhS9PZs2utcAYKth4WmDM5mgYicLGUrbCqv6SDcaNTHUaf3LkSzVRvFiAVrJwA7V3LB9` |
| 1793998800 | XAU | call (cap 4290) | 3900 | `27eiwcTSHQ9HmnFeP1pax6oDUsoW3wx53S87FkRoSwBE` | `MeL4FeHEy5DxaeCMThnu23Mo7sUSCKDVju64i9bMREspb3J793yJmVzucC7WwiGnYadD8rsYrqnpTcPaPBpDtpd` |
| 1793998800 | XAU | put | 3950 | `B3PYLRhWcCcnBnCcPUK4AkMfxYLTuBkvCQyykcDwYkFm` | `6694mJg3qX8Pw66w6rKuhjL4yiF7yXX8fgWntYCkX7CRZyMdCbAaThffruoqVof8VRs47tXSciVDxXHPDW1obu2K` |
| 1793998800 | XAU | call (cap 4345) | 3950 | `Fvv4V15kn97QNjPXWtKmNi4ThctfjcfmNzknuSjMGiDc` | `LhLvxmc5dk2J5Js7o3WEqv2bsrRhYRKYzsQiJpfkKesh5scGB5ovapJc3RaneMhXCyPhLEDJjaDWvZC9zHMFM3Q` |
| 1793998800 | XAU | put | 4000 | `6VzDx2mJzMTf3w9s3VBrtJu6XtV2N1Nv56uMwpZyF5gY` | `5ypGo4ahQYekv2ERTciF2M272hjgpnXJAwKzT1LfQCuCTqesB1hM5qoD3FqkpqYv3aGbh18X35oC15PduNeQCR1V` |
| 1793998800 | XAU | call (cap 4400) | 4000 | `B1eem5xBx3cbvzwqnGZTu118wYTp5UgsJAkWRuKevf2Y` | `29BABSoSqvCC6VYpWGmqxSZjj23gZbW17Mr8waaANz9GEZQRU5cBQWkRuURrekrPg7fRGVS6qLmN1qWMte8qW77s` |
| 1793998800 | XAU | put | 4050 | `6FTcwQTKJH2TPWgN2Ch4G7fbj3hKSEQJK3fAkzM1s6xR` | `4eeQ6voh6PqshgrDXUb5tRzCzDGxLsGcy1yyHqzG3UAnZgUnNVYHZbqCRELiWYktEjQgMded5wEU3dqujPr5DpEu` |
| 1793998800 | XAU | call (cap 4455) | 4050 | `H1JxmmcEKB9AjXcx9nvrdnoBJSBwtvtYR4GiHQJBeXai` | `2RJ2kZqg3SpSssRD3VjRt3iPKHY13YXGkcV9ZNAWvSSkF3VMwFp9hmn9ukFkzRhhNnavkQgaHUbjBVedgJXAonx5` |
| 1793998800 | XAU | put | 4100 | `46FMipQsjAAjFjUBYRThXR93sYDXKCGck4jDPKuFxQNq` | `oZ4VqRefF6xjcQ1vhFFMHgbYjGLeMFjNbddqHmLLzLLHscxsrTvU2k2Zp54oVSBbDBYcXHSLpqtK4cxJ2PrL2ZM` |
| 1793998800 | XAU | call (cap 4510) | 4100 | `2Z1jgp22bhhskNKAzZyu55y1QkWaAsrLuQqZyJKoiaHw` | `3Vmzu2WdnatDnqD3Jx5tFNgu5HecJYdG2MiwEBQogcWjra1FwfErRGJbR36tcWrEiWSmAMWiQ44XR7jy2JFm5cAo` |
| 1793998800 | XAU | put | 4150 | `GxFr9AHmARD2PNev8k5aRN7cG22VNzboGU5yQhm5mc8w` | `3kh2wK8xCNeWaXngkugwMCq5sGNKh7BnMmzuQzqPZc5p82Ucu938Qd9Y2XfCRuoLpJhDSLEVSi6YYxPMrKPnVQg7` |
| 1793998800 | XAU | call (cap 4565) | 4150 | `9zMAutEb6fKswJE5HGZ6xJRh3ttk8p2soMGgxcsf3vZe` | `ty3xAwKY2HHASAY5GcxtNyzgxuQAFioRvcDY6ZqDAf8qtHfLSjDU7a4h3rULCEWuyy7MFVU6SCCSLzSkR9HzdM9` |
| 1793998800 | XAG | put | 48 | `FrNnxxpsR3F16UNWCbVBg8w1LghQ4RQ9DzPfCVGN8CdE` | `2ZSpTYLsyZ2KiYsfiYVMRUmDFksiGP6xjVbPGQBfh4U1gCnFcG9THqbxcq8ZDNT9Ek18iUDHaJ6Wbw3E41dDa5aU` |
| 1793998800 | XAG | call (cap 52.8) | 48 | `E7Zc7AWvVceMBmBbJmTuCVY7tBNVnv8hyxmBgLR1wMcT` | `a1ghNrXn7H44v9VsUB8z1UDd9BAjoVD6eZ3WdYF9vSMD5uaxB3tnyPNCNN2TJAoYcWpZiEs8tM6m7fdj2Tk946A` |
| 1793998800 | XAG | put | 49 | `FTBigV5stJQoDYP6TFyNkeB269D98URY6rqAX5D1sKxq` | `2PBn7hxtCy6M5jiyGi6naDjbvtYK6MFM7nmzWk6baYa5TjKPdSjG2uSr8RmyMeEfk9V6ThQWYxwz2Mo8N4ZYz4C9` |
| 1793998800 | XAG | call (cap 53.9) | 49 | `DtUqKLmUUKru8x5GFeTFzCdJV3d3F2SmUMSnUMY6Mtad` | `2NvXHBiWthHqQYKrGHj8RzkKRyPoXEW82UgjZ6pThSVHsKJ3StTxX2RGNTcoaqHWA5J6PHBUS9EdHQmPqQj4WnTW` |
| 1793998800 | XAG | put | 50 | `3j3aGowHkH2yHKmYV2TsmBETRmkTwaBcsxzhpKjNSduy` | `5CHFWiHMaso9rieVms2YNhAUppD6aecikADd9BtatKm1HufE7AYxBu8enQ8VoweM2ZmTj3NrBQSNMoLFWZHiFwmw` |
| 1793998800 | XAG | call (cap 55) | 50 | `APeCe4xH4ZzRCztsQADvE7GvLW3GeoNN9fRtEKmBAtpF` | `4wpvJBTFViHtRQ7UQPnhsgbr3Ab6iKqnxCHkQhzc3jrNxAZp435eRLQHR1VPAyD7e6qQiJzBSemJ2Q9CgknDV4vL` |
| 1793998800 | XAG | put | 51 | `HayZ7fGLaLjCNFkybCoo42rAG3TdtnhGhcibjYhmtXj6` | `5EKH9VX1jrLY5124XnB8tTYSdmQfKPNfoocQYUcZemgcoZyiaEVNTcAJDRFZRiuw9SRJQy57wte3GbETRDPyXzAE` |
| 1793998800 | XAG | call (cap 56.1) | 51 | `DNptrDVZaA1ZKEFRNgtLTUo39qP1DJBfT6Mo85zQo2XD` | `51Dq2PZHBYEHzeMegWEtxj1yFWAbMd88RcTnMMSAYtJHMvodNKHkK9Qff5fZiT7u9dS6rHRkSHqXTqAwGicWGMG7` |
| 1793998800 | XAG | put | 52 | `92sL1UdGxDkRMtX9sN4f4TSKDpczVuPpHyY4vCKg5ado` | `3jGbebJwvBnBReCC8Kk7KZRHBoC1AGguDTsat2zDFyVATjaTE9t2xUCTp8eLc1SaZpXyotZGQEV2Yc9fGU3jVHQS` |
| 1793998800 | XAG | call (cap 57.2) | 52 | `3BBBDnkZ9Xkab4tu9wcVd2C2msXtYaweCNoBS94qG8FJ` | `Ju5FWuZJTimUPmVfqSbEJTdwk76FBqADRDARZYRmMvSFbfvAJ4gUN2Ev3gFoY9FuMAUigKYpAwtFUcG4jTwyn6v` |

## Faucet-sized XAG series (devnet only)

Circle's devnet faucet gave the maker 20 USDC, less than one contract of any listed series
(the cheapest, an XAG 48/52.8 call at 50 oz, holds 240 USDC). So, on devnet only, `set_config`
lowered the XAG multiplier to 1 oz (`metalbook-cli/devnet-params-faucet.json`; every other
parameter unchanged, the 50 oz series keep 50 oz: live series are immutable) and seven Oct 30
XAG series were listed at 1 oz. The 50 oz XAG series stay unwritten and unposted: they test
the void path. Mainnet keeps the approved multipliers.

| Step | Account | Signature |
| --- | --- | --- |
| `set_config` (XAG multiplier 50 to 1 oz) | config | `TXfQSv9aWV3Laidnx17vUKW8V8PAd4kZJntzqBtGh2GSr5sgyVLjFqrRriYxzYxPGhKzgqSDwxo4xp5ahHipYdR` |
| `create_series` XAG call 48, cap 52.8, 1 oz | `9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK` | `21fNNZ8fgU1uBZnnJCNdNzQJ5E85tvi2WtCPXxSoNx8PU4RoiEtiTpjsQaZyqUfCn4XGxwYfkSJQN3jYG8hGYqFN` |
| `create_series` XAG call 49, cap 53.9, 1 oz | `4BP9Rw5jAprEPqf476bZmZd9W5pibvRFNWKowZeKuAth` | `4TgwDKwVTbx3CkqdWgwhHUCdyVM1jAdkLCi1JswCLvRGAbWUwGTVX32KWSpUeZ7fd71RC8gpL9iikwvDz8Fi4P4y` |
| `create_series` XAG call 50, cap 55, 1 oz | `9HGpi5WbLxWSZfU3WuydbXMPfNvJrdA5T4FC3Dtee57W` | `5kVRkenQGfuRtMs7hsEBHYfBFTJiZ5JU8g7dyeULS2K4YCZTyNFbHip9FAw97WoHKYYUhMTDXqag4eShxwJQdNe7` |
| `create_series` XAG call 51, cap 56.1, 1 oz | `C5CA4egx9AWTVg35Y3k1EhkwfcYMGdGs5hNWDrVM6UTX` | `PyWib7x1J9Kom5uz2Pz9TyYrEjq6QyCSc6sXSq6Us4hKXEbWKwU3iYMZ9SxorwwBvu8o2ttxerrFC7i5URG2HJH` |
| `create_series` XAG call 52, cap 57.2, 1 oz | `Hs2SMW7Uv5TkPzBpZbLmPNuGepAyrUhmh1FfU6Yxtp9U` | `KgEALHig2g3zQaUd3Xyr6GWVGNfNEe7gZPnw8hKUXtatnGavzcUiwB7uPudikkMvuha5E5Z2ohTnLCUa6Tzt3dB` |
| `create_series` XAG put 48, 1 oz | `B5pPdQXhGKnEyWkU2PZZzPV1kio3jaEoQGFBSh4D4uoG` | `23PNT5ghvvUX1fuTXeetaepb4rXMxFyFvG2v6861KU8cFzcgTkznLtsdPSh6KXA48wxi8Row2qnn6eVkmETmYwnf` |
| `create_series` XAG put 50, 1 oz | `BSpa2zeovzEUdd9dZ6LwfNuw3BpBVWpetna9qYDqYG4R` | `5jvFRdWKSTkTQ9RsnosnisNdYvgLtAUaPonXiituUyio2Y5xhjJqPg7D6uw5YcyQZi7K41i9CjAs5WUD4sA2Bsto` |
| `mint` 1 contract of the XAG 48 call (maker) | `9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK` | `4uLd6mpHy7r9tJJDLsYr8Hw42ozJ8tBafnX6Y9D72fVN37pA1CNzeBPDfnp9GQtjTyovjfFGcL5fdQrFjaytDD9v` |
| `mint` 1 contract of the XAG 49 call (maker) | `4BP9Rw5jAprEPqf476bZmZd9W5pibvRFNWKowZeKuAth` | `3erN9yw5hmSYe6KhSEvLSBSeu9jKJQXGWBAh5nPMjvoHSd3aQuNEkkKhU8tUoK9xBVBNYYjrmw1e1XVTf4Zm6WMe` |
| `mint` 1 contract of the XAG 50 call (maker) | `9HGpi5WbLxWSZfU3WuydbXMPfNvJrdA5T4FC3Dtee57W` | `4kNrdXshGRHiL6psZMiXcnBMUKPp9DDHo95AK7T9yqRZVhCyTnzd8D9J6ssqYq8SFLVasrRoN1U5dB14gH781ZHx` |

After the mints each of the three series holds exactly its collateral (`vault_owed` 4.8, 4.9
and 5.0 USDC, one contract and one long token each) and the maker has 5.3 USDC left.

## Receiver posting, live check

`metalbook-keeper --check-post` on the XAU 3800 put (`2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5`)
posted the latest XAU print (3967.935, published 1793363681) through Wormhole and the Pyth
receiver and sampled it as the series' fallback candidate, in 4 seconds:

| Transaction | Signature |
| --- | --- |
| Encoded VAA: create, init, write, verify | `268dv4W1C7ioRiipd7waXEVGjSqnMqDR9KAv5AmScfY5XVAnWZ8s1GPKdAwA8NYbfC6cN1m3f12QUYApuZQtWSBn` |
| `post_update`, `sample_settlement`, `reclaim_rent`, `close_encoded_vaa` | `sMLMjZd7xpmXebmYBJ9bVDMXw8uXFFJBZ7jXw5RYj246M8ykW1Yph2wj6xFhhcwYevMyg7huWhWFVks6uFxGhVA` |

## Settlement of the Oct 30 series

Expiry Friday 2026-10-30 16:00 New York (20:00Z). The keeper (`metalbook-keeper`, milestone 7c)
posted every canonical window slot through Wormhole and the Pyth receiver and settled each
series; all 23 series it ran on settled on the **full window** (method 0, 30 of 30 samples):

| Metal | Settlement price | Series |
| --- | ---: | ---: |
| XAU | 3877.352833 | 16 |
| XAG | 49.184986 | 7 |

Cross-check at 20:15Z: Pyth `/latest` XAU 3882.91, an independent spot quote 3883.50. The ten
50 oz XAG series got no posts and are left to void after `hard_deadline_s` (7 days); they were
voided and closed on Nov 6 (below).

| Series | Terms | Settled (UTC) | `settle` signature |
| --- | --- | --- | --- |
| `9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK` | XAG call 48, cap 52.8, 1 oz (written) | 20:34:02 | `48gRh8LuDg5GwG3CCUjJLdCpe62kpbGAFJykdpdJ8bYRonpyxqqPi4Y4oA9iTndsPGgWqamaTiZrRjGseCSTBt4b` |
| `4BP9Rw5jAprEPqf476bZmZd9W5pibvRFNWKowZeKuAth` | XAG call 49, cap 53.9, 1 oz (written) | 20:18:13 | `5f69PnCxyPGsQEBmakyyV2LgNj2cUB7RApd25MjzxSMGSn535EoGrcAEZ1gcncvBxgqXtfFATR4RfjA3TyjJNQrM` |
| `9HGpi5WbLxWSZfU3WuydbXMPfNvJrdA5T4FC3Dtee57W` | XAG call 50, cap 55, 1 oz (written) | 20:38:39 | `3GnDJWjELmJFauDZhdLUKggEYDERahMdYGYGYs1CYiqght86DmqK6PUizUTKUpEe195zM9WzWoMbU4SSLhfDKoN6` |
| `C5CA4egx9AWTVg35Y3k1EhkwfcYMGdGs5hNWDrVM6UTX` | XAG call 51, cap 56.1, 1 oz | 21:05:07 | `2df1bTfC7z2eVnFvEVr4CsSWNEkEVaCBvDiZvFtsW4GKNkKBnXXrV1VNLJADuxQGB6BwmCtm7DmSGDKR5QEGSezq` |
| `Hs2SMW7Uv5TkPzBpZbLmPNuGepAyrUhmh1FfU6Yxtp9U` | XAG call 52, cap 57.2, 1 oz | 21:24:05 | `VA8xVhqrDfu9qYW6QR5ZYtNZjz5rQHYaxTPFjQhAA113XxnPXkhWmZKKAmwbBQtLUQuwrnQYmbLnz362mpVTvuN` |
| `B5pPdQXhGKnEyWkU2PZZzPV1kio3jaEoQGFBSh4D4uoG` | XAG put 48, 1 oz | 20:51:01 | `222aBwD7MmsaXyGAEhSJPgveDtAkn3XBnunNFLHPVt1313KKJ2qtTWahuVD5vdfBdTRziwE9T8bmTtyiFSfRm9CD` |
| `BSpa2zeovzEUdd9dZ6LwfNuw3BpBVWpetna9qYDqYG4R` | XAG put 50, 1 oz | 20:55:39 | `3KwF4QLbZwUc2Wxtwabfn1i8fbvagacF2CGoaLfoiJTGkSoKUXBW9GFhChcDzcbNAGDUkAMuL4VUeHWsbyj79rnF` |
| `BvhmabWKVnDWacexfQw58U1uGbU3HLDV4AfHKquuaBbi` | XAU call 3800, cap 4180, 1 oz | 21:00:22 | `3AM2k9B1j2UUhT5J7Y19bfxDEG2MJ6WYb3mFUhUiooYj8XsuWvQA1eWxd7f4q9YuWADb4QvPBtbYcrXwuahjUuf4` |
| `Dr5DN1cyYkNq4Xe9cKEud7cXDPjk57ERhhzvUQdBXHA` | XAU call 3850, cap 4235, 1 oz | 21:09:43 | `47HExj84syndWwLrMZJrrJGTn52xDTfrUGoTZqhvAtNcRFk9XGgtgB5sD8nv77fRhkjJ2Fs5hG6SLCffx7VPz1n8` |
| `4W67aaLu1gUYwBszn9Zubt1hAq2wy4SL5N7XRUczF9St` | XAU call 3900, cap 4290, 1 oz | 20:26:18 | `5evkDXfADt2VkLs79RTxbtrQBMUc3nrXahrvYxtw2vYcEAtPcyBWbtrYML5MYKNQ5562jrBHLPZrAXzFNtRaRBt4` |
| `JvKCAALDRAxd77sCLw1as17CQDXbvsg2u4ydbtGxQAi` | XAU call 3950, cap 4345, 1 oz | 21:28:47 | `6SDdULQQkoS3NRu64zgnY9CQjYMpTtpufFMd8wmrMRDvB1Mr2jNBTj4nde5YjYJthY1vCxFUZLcpSpXtFsYVT8s` |
| `92xnofbYSfeSqvoxrdNoPfeU9kpfr7FDu8JeijgasQiw` | XAU call 4000, cap 4400, 1 oz | 20:26:18 | `4DCrNCYYGgKemmiyzx7r7wr5getH39NpD47siAvMLLb5DJTHNwM2B5HJVtaahzUXcXzLnTGmTCcsJtsHBJ7mPRsb` |
| `6ZbdqcGTddC3SoTtggHEWWyDj4XJsFGwU58GVZmktmyM` | XAU call 4050, cap 4455, 1 oz | 20:09:01 | `wf9a168Gqw9s1naeXppLv8p5itUCtey8vnfQ7WuFdLtvARDELcsMTSA8rPo9vf68XdcP23eRwqSbkAQFL4YVKCB` |
| `6qg3wruL2TocFw9f4Q3CXA8mmWx6GKsGeMTUphPJgF5H` | XAU call 4100, cap 4510, 1 oz | 20:31:18 | `u82FBLMyzf4TwtSDsUH4NnhwVEA3RnQvCkYm9tYxrPcoHQTYGNaNkrXdzeTN3juVXmM6ftyPkYu6tR1FgtayvU5` |
| `3hhKKRXq8bJne9aPH2P5vtDjnaY1neJuTRpVhzDy53Jq` | XAU call 4150, cap 4565, 1 oz | 20:13:20 | `2z2jo62iXCGGoCDpGiqG72SDMXeie2F7k8qzq5XZooCdSdyk85qtDaBBkMWr3hoDLmTBHmu47fCU8AdFQyVX7kdC` |
| `2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5` | XAU put 3800, 1 oz | 20:08:16 | `3P9qL5Fn5aSTepwzXU55Cdg3jkarQto7fmaQH7fUHDohwuHEELj26SPL5M5N4XG9ZqTPaNuWqSpHJkxdPfxQSXGv` |
| `ANtq1DXY6rg92dag8sLHLzm8dMG5t2Pn6CZzZw145oon` | XAU put 3850, 1 oz | 21:31:53 | `3bDF1qLi9uKRovCGpExN49tS4R3hvv3weWoEGNo7LuMR23LbdLWjRSSY6n4tntRGiax1YdYGh9ogE8dyM9F25o5` |
| `5vvy5enzqBm5zfZnUFnDs8K5AG8JEEwNbspvV2CDusbN` | XAU put 3900, 1 oz | 20:29:10 | `23NTtpZtSHwUEBwe8omE8e55k7iDYQRYnv54DEfPiij54fEhCDoUDG14EwNuGGFrfELLLWUVXQ9h3nrRATAVEPoW` |
| `AqnB9PpgUAhW4tmqzt2Srzsgdfm2BzU6m2jbbfYYpBi4` | XAU put 3950, 1 oz | 20:46:19 | `2zDhKAaRiPxKXMfMrnPeRAtiFrWSrbAed8ZgZsjo2LcRF4zJNnXXtV3yqGo6a69BcQjYmi6yHKefwSV7Vzif8T5c` |
| `4u74R8Zn88TTxFHaDBVifQKSHgyJbsGCo9ndhG4raXhv` | XAU put 4000, 1 oz | 20:22:36 | `4pFVZ5UbpU3gSAiAVLS92fy6rwtxjV4XvAo6aqne7fsMiJMkPcgqxDHpStMjjfzqncQrmaZWh5guZzMoq7MvKFE2` |
| `8Lv5WwKejsFCrawBAvVvUgEdsSmCBaARevXgLzvSUCAU` | XAU put 4050, 1 oz | 20:17:35 | `2sfz3V1TCjPScaswNTrGmMBY53aaJ5mytu3xpCuA1zRdggRfzUSYv2rr5KhjP8oRLm5ornpcEMLtH5WXZ2Xb7CZd` |
| `Dw3skzBfb9S3kBrRnAvSkvGKoZcRbFy178yZ93WSiL1y` | XAU put 4100, 1 oz | 21:14:22 | `3FYpgdCDi7hfbbsojy8QB88vZHaHygdmJ73tHZ876W2KsM3KHYYsUpDmf4xy4CYX6H3fSQ5BorshXrYCnE3DoNKA` |
| `EfyCdzYUmTY8ie6XtbHpQAgzA1bBZdhC47wK6Ez6JyRU` | XAU put 4150, 1 oz | 21:19:07 | `4vbH6n7k4smKXgLYRRFh3V1322GHjSQeDUNvsQZD2fGAnadYhNKC1hQywow9ivuQuSMnv4821f3pxusbGhpdk11m` |

### The written XAG calls: redeem and claim

The maker was writer and holder of one contract of each; payouts round down and the writer
keeps the dust, so payout plus residual is exactly the collateral of each call:

| Call | Payout | Residual | `redeem` | `claim_writer` |
| --- | ---: | ---: | --- | --- |
| 48/52.8 | 1.184986 | 3.615014 | `5tWoFRk7GEmLA8RNzHL2a5dZq9YDT8TpxeqkQxDRvt8VHsZQPs7aLqhXwRT4PPUJRzFurUnvprzMj1vYbRac3CV1` | `5qEJqQ2Pny4PgqvwiGGS63t6z9Zv9ptHNGqiwpXaLS9nH7hNNH3FGa2QX7rpv2EB1dBVBnH7wmfPELnyp5E42ZW4` |
| 49/53.9 | 0.184986 | 4.715014 | `5swQp7mo3FxdL92BndzcYrEse85YqciacY2FmWZ9iMN4Zy87xGwUs2gXutPripa2gM8a4Xxhf374x5aoJ19Mc3GY` | `5iRSMeu7xhGk7AZpvHw3iZPmbiWBHYH9jD4yA5vKwisUR2iBHsTYghoYSgRWcr95hiTnzTAPEhNQmYn8ieAkJjLU` |
| 50/55 | 0 | 5.0 | `2CTUjwgurNGJwwoJEighSom2X5izuj96G9wXN7xoCBNqq68iLBr7j4AgTNr9btytnNwUofFeepwBXGPfYLGNQpyr` | `4PGMJNAEUFUjCCK26CBVj3TKNiYEt2fvjzoed3JkB1jZc6mUSky3tDjBuZdP5sdvwU8tvcLpj9XEmtAkg6QMG9t` |

The maker's USDC went from 25.3 to 40.0 (+14.7, the collateral of the three calls) and every
vault owes 0.

### Keeper run

The keeper ran on the Studio from `~/tmp/worktrees/metalbook/keeper-run/` (binary and logs
there, outside the repository), over the 16 XAU and the 7 XAG 1 oz series, and was stopped by
PID after the last settlement (21:32Z):

| PID | Build | From to (UTC) | Why it ended |
| --- | --- | --- | --- |
| 97954 | `6af18ee` | 12:57 to 16:57 | moved out of `/private/tmp` |
| 39808 | `6af18ee` | 16:57 to 19:34 | Hermes 429 storm, hotfix #30 |
| 81804 | `f935b1d` (#30) | 19:34 to 19:39 | pacing, #31 |
| 88628 | `417daac` (#31) | 19:39 to 19:44 | requests for future slots, #32 |
| 96108 | `c0ba97a` (#32) | 19:45 to 21:32 | done |
| 16099 | `c0ba97a`, catch-up of the 7 series ticked before expiry | 20:02 to 20:43 | done |

What the live window taught (each fixed and merged during the window): a per-request 429
retry let every series ask Hermes again and kept the key inside its one-minute window (#30,
a source-wide 61 s cooldown); bursts of backfill requests needed pacing (#31, 10 s between
requests); the keeper queried Hermes for slots still in the future, uncached 404s that
stalled every series under pacing (#32). Two keepers sharing one payer interfered: one's
orphan scan closed the other's in-flight encoded VAA, so three posts failed with the
receiver's `WrongVaaOwner` and were retried (one payer per keeper from now on). Throughput
was bound by devnet confirmations, about 6 slot posts a minute.

## Nov 6 drill: void, baseline and restart (P3)

`docs/PLAN-improvements.md` P3, run 2026-11-06 with the binaries of `main` at `275525d`
(`metalbook-admin`, `metalbook-keeper`), copied to `~/tmp/worktrees/metalbook/keeper-run/`
outside the repository, payer `77ieYLKspRnGzAt1AK6ZVsttnTY6T7EY2k7CpTZeCPPV`. The void and
restart runbooks (`docs/runbooks/void.md`, `docs/runbooks/keeper-restart.md`) were followed
as written; the one addition was saving each void series' `show-series` output before its
close, for the phase 2 records.

### The void path: the ten Oct 30 XAG 50 oz series

Expiry 1793390400, so the void opened at 1793995800 (20:10Z). Before the drill all ten were
`active`, with `open` 0 and no candidate: no keeper had ticked them, so the cascade had
nothing to settle on. `settle` was sent at 20:10:30Z and gave `state` `voided`, `settle_method`
`void` and 0 samples on each. `longs_outstanding` and `writers_unclaimed` were 0, so step 3
of the runbook was empty and `close_series` followed at once. Each close returned 0.004699 SOL
of rent (0.004694 net of its fee), 0.04694 SOL for the ten; with the ten `settle` fees the
payer gained 0.04689 SOL. No error and no retry.

| Series | Terms | `settle` (void) | `close_series` |
| --- | --- | --- | --- |
| `7WStW2SFVLeeTYrBCMszTZLgMkgDH2YJRABEHR7iiTao` | XAG put 48, 50 oz | `5cB85FQC8DNYGpkM4ZC1r1q1RRMx6LhxpbGpSqJQKSCDhHsWhSEgndsPMoRDXkCuHRDSD8Tkr7RKa3x5tg2A2rW2` | `2jkm6H8fLYn6iCdJ4kyXRZsMHefJwhAcSYQRuoE44NYNEm4dcWaGkXEhNLteARyrBYje7jEbGb5nKobcJvAYmt5` |
| `7EUNY3M7579s2M4UnmDS7QscJK61a5rrpPN9WQNn5wVw` | XAG call 48 cap 52.8, 50 oz | `64xReQd5rXYzWjpAgWZYQgk2HXyiKPqA2kPPvB1tLRTqMnj5s2gT4Sw4Kjv8sUSRS1FYAd17ewBY31mMwiN4Zknx` | `3bZHvzqKWSCSnde6H9Huj3a16KewGEx8Vxf8BN6f9CJBvhy8H5Bn1TRKFYukJvzBKXtDmbQpZ3wPZUvU8EW5Uohg` |
| `7gDF4kJGSQMzHcqoWB3FVqEvV8AuHH53JC5r1jBRN377` | XAG put 49, 50 oz | `2cEoTMERzrx91Q9y8szP9zaeCAcAW9KDm2hXRrELrvwrkrGxgipXpQqivb7Q7H1ptP2remY7iyhm83ukM7WqZ4kz` | `39CiGtVL1SZDwqNezcwZ1C3VQkWtLHL7UDKbmGqNPPFvo8KPmnFLfiY8Syt4675YozDUHkcUWmV8rsxj7kqSJwx4` |
| `HbovhXFHPTJDj3EkCiEyzaSKsXCocxH5ot5djtcGERLW` | XAG call 49 cap 53.9, 50 oz | `3VuTVXUCZuZNViue4Be3wxRwm8CkvjcKzk7dTYmf5LzNPuJTsQA1ftjhdGVfHDttgj3hG9gu2xC6XmM5VTdw7HeL` | `4hdFR6jnzWgaXXax2RqQsQTXTc6X4XnikRiuP1RQX5QXXrgQrtaWGKyEVShWNUmtBhhQPqNEHXKvCpp6oK4siZew` |
| `4F2hdKE6gJ7TvMBq14pWgsBfKb3VxhTNA96CbYkqsWHn` | XAG put 50, 50 oz | `57RNqoHgi9rnf8EdV4UGscAitsNPViM2pC6JAaHqoRP7bpu4ZoTCXhBBDZTiGDYa3uEj7xNs3Q9SMvXKf9zgzNh5` | `2h2oKFhcFVkDGR5dYTsGM6Sb8k6585KWGDZRM3oGthTyR5AAiWKu6LtypqSS1PYXNFj7wikccLtotuvbEZNMsMaD` |
| `84uyQjzN6MkRi7XCx8q1xEqqhQFYavRAt119jMXXn4BY` | XAG call 50 cap 55, 50 oz | `3FP8xLPnRSGL4JTQgWxVYHM4bed93KAnGTXnZcWiWnFgkwXG9g27SP47Sa71jbMdGs17cspGgg1PedRuRwXfA2J2` | `41Z44peNejYYZ9RuR1oEaQjh17mq7LEDTcxtapC4WxZwPFj9145MM7BhZpZLHZVRNcdd9PE2bkYn7W7TRVpdeEWw` |
| `GZz1xtQKoUjAj7v3x63PBgCDv1UexqdmK25HuyDqzmWJ` | XAG put 51, 50 oz | `3XikAp5M7APzBQbUUmEaroj8rKCxSvgCrd6RiZsRPGDpQh2sqwn3ubBySBWQYQcmjX3HMd38CzZPELdKYi91k3Km` | `2VY5uY8d6uvNJiZbRALf21tzHifnDBbXvXnkBKCeAxxfH6fdZXnr12FXeW1iBsv4ZjKANMsFymWBMpFSA7xrQKx5` |
| `Ew94BcV7MYXiNx77yooBXteEe4XsxykZodU7Xg5MkpPD` | XAG call 51 cap 56.1, 50 oz | `38PgLxQPJ8XjVohZAzYunHRHvxobraMSf1J62PFEnYayfBJT3E9AYQ7sYnqSW4k1FxAXK52FZyhZfX99s39kKM6R` | `5gcDoMkdaDWyPCXw7nFYDc1krvHtvp1NgoN3avAcMw9ERQchKe2jjBALeda5EFibHytHKe5Vx6eVNkWhPzBzvZKV` |
| `6SYM398CRzH6j2u9gikyGCrSeRtdR3QK3pCCkrMFik31` | XAG put 52, 50 oz | `5Xv6QRmBacjfv2y4EQ13bxCCVz1qtmkESrTfGi9CTXi78XkyURvbKAHBaU43t4gWZkRfRnyYngSwegmqpbFEHEmg` | `42ZEvbmfJysjPKvMW3q81pAzbnyhb1VWyaQsXassQr1FURez4Zokm1NEhJuPbu4z2BLXzCdUkgFRFXwZv8SetiEq` |
| `Df5oVmx94PJM5BQEmk2QeWr7DEgaHaZ8TpBwzyDfaMX` | XAG call 52 cap 57.2, 50 oz | `4PHeJhSD83FMrcEdxJdm6gMY2RAUH8o36JU4qhxXtPCp2wUPwk3XDHZ4rycSgLUKs6WXVDLAEGdiUNtGmrUbhDyd` | `67c2nBrpRhAw53F3tFv2g3ZYB1ZHW5XeT7ANEyf7NF7JtAEkaiVkx4miKFoSWhFtjUBxp4sH8nvFX52vRfWJqzfx` |

### Baseline: the 26 Nov 6 series, unbatched

Expiry 1793998800 (Nov 6, 21:00Z), window 20:30Z to 21:00Z. 16 XAU and 10 XAG series, all
1 oz, none written (`open` 0), so no redemption or claim follows. The keeper ran unbatched
(`--no-batch`, the shape before P1), `--hermes-interval-s 12`, `--interval-s 60`, with an
explicit `--series` list of the 26 and `--exclude` for the ten void series.

Every series settled on the **full window**, method `window`, 30 of 30 samples:

| Metal | Settlement price | Series |
| --- | ---: | ---: |
| XAU | 4065.3453 | 16 |
| XAG | 51.951246 | 10 |

Throughput is the finding. One post per series and slot (780 slot posts, plus the next-print
links and fallbacks) ran at about 3.5 posts a minute, bound by devnet confirmations, so only
112 slots were on chain when the window closed and 12 series had none. The keeper kept
backfilling (a slot can be posted after its time), and nobody else settled, so no subset was
settled early. First `settle` 21:10:11Z, which is `window_end + D`; last 23:40:31Z: **3 h 10 min
from the first post (about 20:30Z) to the last settlement**, against the 30 min window.

| Measure | Value |
| --- | --- |
| Transactions, restarted process (passes 1, 2 and 11) | 2,682 sent, 2,682 confirmed, 0 failed |
| Bytes attempted (restarted process) | 1,627,694; largest transaction 871 bytes |
| Longest pass | pass 1, 9,358.5 s (2,360 transactions) |
| Hermes requests (restarted process) | 100: 98 answered 200, 2 connection resets (retried next pass), 0 × 429, 0 × 404 |
| SOL spent by the keeper | 0.02612 (payer 66.1221382 after the void, 66.0960178 at the end) |
| Encoded VAAs left open | 1, closed by the leftover scan; 0 open afterwards (`getProgramAccounts` on the Wormhole program with the payer as write authority) |

The pre-restart process posted 46 slots and was stopped mid-pass, so its transaction count
was never printed; the table counts the restarted process only. This is the baseline P1's
batching is measured against on Nov 13 (24 series: 3 posts per print instead of 24).

| Series | Terms | Settled (UTC) | `settle` signature |
| --- | --- | --- | --- |
| `E7Zc7AWvVceMBmBbJmTuCVY7tBNVnv8hyxmBgLR1wMcT` | XAG call 48, cap 52.8, 1 oz | 22:14:02 | `5ZU6QEF3EFDSBowjbNkM25BD4EjwEfspRrw8UDUZXbgKEUbSPx7FGEygdcAWxQi4uXUtbJXaKq8jam76Hjqp3T3i` |
| `DtUqKLmUUKru8x5GFeTFzCdJV3d3F2SmUMSnUMY6Mtad` | XAG call 49, cap 53.9, 1 oz | 22:06:55 | `5sCaFVAtfhRyCPwXJYSf9vMnQ2iQ6n4f2kbnFbSFRrsA8bDuHJjh2bvqVVajmUJvwW25J5D6DLrE7azJHKYYrQiH` |
| `APeCe4xH4ZzRCztsQADvE7GvLW3GeoNN9fRtEKmBAtpF` | XAG call 50, cap 55, 1 oz | 21:41:56 | `5zYuatMNSKUhBkGE1Bzrch8tyrkXQ33JTHTyrKq65HVfxAM8xFmtdFsh3E1jRgGvQvPi2JhnovG2Rti9HdC5ArtL` |
| `DNptrDVZaA1ZKEFRNgtLTUo39qP1DJBfT6Mo85zQo2XD` | XAG call 51, cap 56.1, 1 oz | 21:59:22 | `5PqEFCdC2PFx6RoKk3fV6rQiioeKezfmBLLDF1SFQbrtUfbmytD1TYKTfx5UCBiLATqRB6PhBiU4YSA5RYedVnhg` |
| `3BBBDnkZ9Xkab4tu9wcVd2C2msXtYaweCNoBS94qG8FJ` | XAG call 52, cap 57.2, 1 oz | 23:31:36 | `616NADDop4Hkfa72a63E7my3aa9Komn8HLMpe5M9Qc73umXhamj8VX5cs2PMcAYbMuTc3LTRzLEFsUJykswnPPTE` |
| `FrNnxxpsR3F16UNWCbVBg8w1LghQ4RQ9DzPfCVGN8CdE` | XAG put 48, 1 oz | 22:35:21 | `Di76FnVhGG9HWizpuxbgBWxEBBTzU52RxY12PksNm371oc1jDKSn7bex8ZpcHMDL6ibmNAxj5D38VX4hoQXBXVj` |
| `FTBigV5stJQoDYP6TFyNkeB269D98URY6rqAX5D1sKxq` | XAG put 49, 1 oz | 22:28:22 | `dCUsfLTf41uk5XiVjrKhdyprxNhTvDEcdMLBrZT863KGiw7HGsCrBGkKFFp38UTWsgBTKGgon5TdnuAhbtDwAmy` |
| `3j3aGowHkH2yHKmYV2TsmBETRmkTwaBcsxzhpKjNSduy` | XAG put 50, 1 oz | 23:37:30 | `qX4KBDgpmXe4XzJjyPWBCUTQmZBMmGoJHs4MQGU8mccFo7nzoWiwFsf9w8FwEN6C95dvoptXoHGP8sZm84Ea6tq` |
| `HayZ7fGLaLjCNFkybCoo42rAG3TdtnhGhcibjYhmtXj6` | XAG put 51, 1 oz | 23:11:34 | `52fAeS5NDT4mngc3JBAu1hT2frQWYnXwNfTdj2DrnytzvaY2MHmqUcYbrtnzi9aEbxS5h54v8Jh1ccMHCfNnhjuj` |
| `92sL1UdGxDkRMtX9sN4f4TSKDpczVuPpHyY4vCKg5ado` | XAG put 52, 1 oz | 21:29:48 | `4rM1MtvTR81jo7NgzeCmjzWLwu1Z8LukyF4pFHs1fCFvrnktnxqPPuuajiAi1hxXDK4Cxcxx8q6yA1DjKjMHftqH` |
| `3F7YhT2Qfx5ASHfYNyHyuefZqbD4YvTE47caa9Fz6uWr` | XAU call 3800, cap 4180, 1 oz | 23:35:19 | `4GksYax3vehfmxXMkUKNUM3fbajPXD7tNgUr6eCaPeWjQkD2Vn7QoyDhGgPgGfDmVLYfAEHxXWnRiTWaJFp8dYUY` |
| `F4katJN6EAMXoHmUTn3jABafiRk8XgxoY5jQPjfhBTVZ` | XAU call 3850, cap 4235, 1 oz | 22:21:16 | `2uAeDFHMViGaEdx1KUF6jaNfpwLxj9Dzn3QkpgjGy7hqdVQGvsXKVTCq8ZqdwWcgMb2SFyBkZ82ubWVSnJwFak38` |
| `27eiwcTSHQ9HmnFeP1pax6oDUsoW3wx53S87FkRoSwBE` | XAU call 3900, cap 4290, 1 oz | 23:23:50 | `vcteh3g89BWM417mCrxWNiHeb2EQpyeDEh56nLpkSqkphDk79uf24zCQ9wmwy3gKu9whHBJzS6kx3m1AfR5J5M9` |
| `Fvv4V15kn97QNjPXWtKmNi4ThctfjcfmNzknuSjMGiDc` | XAU call 3950, cap 4345, 1 oz | 22:42:20 | `5aEJfJaZW8a213HqgFm8nnszgQJ4hPtXUrRyQU3WJksVU9pv3oZ5miC3dtHkuCGUCyLfGVMFG2w9HKKUbL97bTXY` |
| `B1eem5xBx3cbvzwqnGZTu118wYTp5UgsJAkWRuKevf2Y` | XAU call 4000, cap 4400, 1 oz | 21:47:49 | `4d9AA1KvZcmdXSC94fjq1seLGqEDwFqtSUySuvSSmTjEdqqD4qjWExCtTgZy3HboqxMLZa7E2EewRDUReYMhuWt1` |
| `H1JxmmcEKB9AjXcx9nvrdnoBJSBwtvtYR4GiHQJBeXai` | XAU call 4050, cap 4455, 1 oz | 23:04:21 | `5JWSCJaFNQQvnP9TrC3LcaqrWyJtc7dJhffxei8EnV7RJMFS3eoYBToGPU8WZMFCsX1BgWgXmh3TTstJENeEKjf6` |
| `2Z1jgp22bhhskNKAzZyu55y1QkWaAsrLuQqZyJKoiaHw` | XAU call 4100, cap 4510, 1 oz | 23:28:02 | `9Ph2okboYxy1B6hLBYbSLqFqa4TsWCXH9VA4g5CYgeqJaAXrwuRSjkFQLU7ddjLV8TJBvGDCqpD75K1VuqwXDB7` |
| `9zMAutEb6fKswJE5HGZ6xJRh3ttk8p2soMGgxcsf3vZe` | XAU call 4150, cap 4565, 1 oz | 21:36:04 | `43PefRLt94SzJ2iTizhoYx4B8X3abDy5A7BpWL4bupaPeeSxqrwHTr4ZnNHxXKXmHdUtZCWuzpd3c8XgsaB3VG4x` |
| `JAUr2AcDZq1YH11fBjmKwu2FQVKm6Jh1h2V6r4exy5iS` | XAU put 3800, 1 oz | 23:18:21 | `5etL77QbQgS49t7W1c7h6FV1YUXbU7448C9ZTV6QEBy9vTHZnpMAmG8Q8EsHskkWPvphrxq9NLpJo5BEAbQs8gNs` |
| `8qTijtsquv5DEf3UJzwCYM4xnvtvJBKBtR2X3UVjr6QY` | XAU put 3850, 1 oz | 21:23:25 | `447WtrHTgsPcSqcqjhBrDZ5iQRPbiGQ5BS49HWbUcurQHWBk1gFYKyJu28x7eScmQX8SGvSKDnjB7EDqG7WW1inW` |
| `GCbeoaTa1yF4azv8hakR8jQ5CGVwmm6HmfHJ15SBRxbt` | XAU put 3900, 1 oz | 22:50:06 | `5w3CnQjbvYJhauviivjPtD83ejA14MQXzKpyLtvFfMkeBPe5P12rHswmPh7c5TpfWdeQapKGCwpXLrE8wX7xtfqk` |
| `B3PYLRhWcCcnBnCcPUK4AkMfxYLTuBkvCQyykcDwYkFm` | XAU put 3950, 1 oz | 21:53:26 | `2h7m8YHv42XdL1GZSHNrzdY8iVznPBYnrh2wFgLM3rcrypbJo29QXnE2v4zuJnXysNUzBXcgp3vcoB3a6mCndxa9` |
| `6VzDx2mJzMTf3w9s3VBrtJu6XtV2N1Nv56uMwpZyF5gY` | XAU put 4000, 1 oz | 21:16:54 | `249afCDwyrGEXRKMGHj36BrdXXuDVNcVUKgS3JsYCwkKwDkA8hVdKdwyq4uFhpxoJWEqn28PP2ju9Fi7itN2VoWn` |
| `6FTcwQTKJH2TPWgN2Ch4G7fbj3hKSEQJK3fAkzM1s6xR` | XAU put 4050, 1 oz | 21:10:11 | `2S9fD2dngoGftUQAECUwsE7VAzNPYhFTNTzfsWsGmnT8rz4S4nVrCeuhUtMtTmDiejcjZyodHJ2C5LKWxsKa9rt9` |
| `46FMipQsjAAjFjUBYRThXR93sYDXKCGck4jDPKuFxQNq` | XAU put 4100, 1 oz | 23:40:31 | `5FLt58d1aBgGSAzvYg7WEYk37qCSpPQ1G43mWDuDcpFxbyovQCH9UFHwQk7oWpGPGXk87P2vmUFqF8qkBCBJTwZJ` |
| `GxFr9AHmARD2PNev8k5aRN7cG22VNzboGU5yQhm5mc8w` | XAU put 4150, 1 oz | 22:57:11 | `31w7cWJMqBV3uJejz9ToDfdkZPDGg4FhniWZjwdNNGDkcnGUeQX2jVjjeWZve9pE2irxUc1XiHmxN7S13LGtPVs5` |

### Restart drill

| Step | Time (UTC) | PID |
| --- | --- | --- |
| Keeper started | 20:24:05 | 49339 |
| `kill 49339` (checked with `ps -p` first) | 20:40:20 | 49339 |
| Restarted with the same flags | 20:42:22 (122 s down, under `D` = 600 s) | 81149 |
| Stopped after the last settlement | 2026-11-07 02:04:11 | 81149 |

Checks from the runbook: no payer-lock error at the restart; the new process resumed with
`slots +N` on its first series; the leftover scan closed the one encoded VAA the stopped
process had left open; and every series still settled on the full window. The restart cost
nothing visible in the result, because the slots missed while it was down were backfilled
like the rest.

### Records this section also carries

**Nov 13 listing** (`list-week`, 2026-11-02, 1 oz, expiry 1794603600), from the chain (each
series' first transaction). `B66M…Zx21` was listed earlier for quotebook's I9 demo (quotebook
PR #23); it is the only written one (3 longs).

| Series | Terms | Listed (UTC) | Signature |
| --- | --- | --- | --- |
| `ZEmgA2iSdz2ruWcX6ARi8jZR6MEiiCpiDjzBGiVZAdW` | XAG call 48, cap 52.8 | 2026-11-02 09:23:36 | `4Bu353hAMVGDfWWE1Hcr8r9K7CRQ51YhdQWorjEdu6sFWd7jG4uvB5SPRpqkSAbsJWEz8756j6vxk5jcg68YVyb3` |
| `R1KQh6RoAbymFvXZ3uxxCzqKshDdNRJzoVar2KgiAsW` | XAG call 49, cap 53.9 | 2026-11-02 09:23:39 | `3xA72d714spd2Wh5GZKLzAwhYzB4YKMtkbt1aLBVes4JRLhu9bH7FFN9SG4WzQxCQsU75JQXVV8Uq3LLqvwTmytW` |
| `B66MftMja6L6g2XFNLWHUWE2Rjq1yTHtToWQTvxsZx21` | XAG call 50, cap 55 | 2026-11-02 07:43:47 | `hcGM3R9MTE1RyELZUhEAuKekXgke2Uzu3ExZfaeVj49UmzpTpQBmX3mVQKDr1apcqk8A5gF2CDRyx3XvFJiKKVZ` |
| `6NuEV8mbELdVAehtszXJpsHP11mmwaMDkoqWgwMXuZ6r` | XAG call 51, cap 56.1 | 2026-11-02 09:23:52 | `3AC43MqopVXnBoaAYVMsCu48w4FfgcHLwikr27G9fXuqvzMZ2yGno4LgrYYNAFH8tfhZey6zLs8E3BavY8umbGU` |
| `HzUsyCk2AAZ1Zy7DS6KLYLB1bprBjsSjmVmgLc1CpWxS` | XAG call 52, cap 57.2 | 2026-11-02 09:24:04 | `2GfGmQUFRRTunwUpswzuwhSMLCoQboy3YznRHxSWxJRiFh4PRkW927id2QaUBELt2cqgN9q7UwsCfuwrjGvL2zHj` |
| `Tv6uNo4Myw8spocwLq8nzzAPdsZRRdSx1R93uFJKGSa` | XAG put 48 | 2026-11-02 09:23:35 | `2vbgPxxMTiGhzE8HaCC6j7bcWTgaDpA4Tm5giKqEf2qpn76HKoKsdzF7UwXEHrbgAEpiuVm5DwHvEW3ZUPuGd1Xe` |
| `4PUCmxv6meHA76hqNUwjyZDuDQfKAYLBhp1JBQ5XnGNS` | XAG put 49 | 2026-11-02 09:23:38 | `4Y7aKjmLry6wK3rBmUnAu4MvYKt6NyzhCczzQi13RZ6tNggZhkK5ibwDngyrHHi8Jydoqd2fB35iZXCcYXQbDTcB` |
| `AzC5aXQrMcbxVXfeXMGTsCFok8ncSpFmj2qhExKV7zUB` | XAG put 50 | 2026-11-02 09:23:49 | `v6h8qTbpGpUfh2QVxLh2pAU4btsMTKK9fV8tPiftNsXkuMXG8cMLThBa8vPMWBstPKZEfmeq9GYu8TVGaj9wQXF` |
| `7YaPRTML6KUAtSWbF6WXmtKKvLmEn4hqZM3cQiS7uxMz` | XAG put 51 | 2026-11-02 09:23:50 | `56BSpLLLsyhNkXe6k11QpFHu28mRjPtYGqyTj8Akfwb8TnsBwUM4hdrNHMVUCrC4GsQNtKBwoCp2qkB8kivqogGG` |
| `HgHcFZ7fc3DtE4CJFcJYsW4fPuv8nbBxZZAES4zsWjqJ` | XAG put 52 | 2026-11-02 09:23:53 | `4YysNsR7SoY2cxW37PVEqPPhVhEkAAkyNjEHz1nRAyTWWwFLSsPx9sJDT9s6LyeJCFvyC9UBMosbnd81NfGY1X7e` |
| `HrJPbjwHUzFrzaeLTkxXZkPyyoeE54Ns6X1onEff1ETp` | XAU call 3750, cap 4125 | 2026-11-02 09:22:39 | `4djFxdqatYcysobiDVsjbRgoAZsRuGWCgwZphTbBgxBNN27sXdgCY2wFoxGGkMw4ByBnoc1WC2yqbE1BLNWrW1jL` |
| `CAdP7LfbGa5jugWBGCUvFXGDAqrWeukYtG8sSQPFVYUo` | XAU call 3800, cap 4180 | 2026-11-02 09:22:42 | `5EqMYMV3F2pn1erbNTq3ii8yVfGZytfx7G4RgdYZckCJ4afx1FDxZXPnhUSuUWHiVEhfasYRzJJisMnJmL2gdNor` |
| `51wa5Hh6ziwwWymf1pdENWiweSTXZrufHm1didRcB1nr` | XAU call 3850, cap 4235 | 2026-11-02 09:22:54 | `3Kw2g6VVLDhP9JVYHr3bxT3EpqMEkeyjf5QEsovcotA7g4HH9cXMG3vHMd7DywpjgUEwUF8AN2hssQ2kHQbgRn7c` |
| `42iGoaVGrcSYcad49gVojG7XpSLXZyjzbwPZGigUcQto` | XAU call 3900, cap 4290 | 2026-11-02 09:22:56 | `4cxGoFi6T8Sk5wLjmyDdreCSXxRxXMyEtMCPVRXFtQPGCPt1ckzz2x9BN7N9reoPCvXcywtvsm8A9RyR4SdW69A6` |
| `EMRP88Z7QNzYCfMiaDBXkjM2zgn76p6CXV4nUVBJY9sp` | XAU call 3950, cap 4345 | 2026-11-02 09:23:08 | `4enzUfmNwL8F5y9iW4q7S5YRf7RbmnoU53mqLH2LP4xptoagURtP5sBh1WMLKjEosS65o15psrSC1vRMUa5QyF8E` |
| `H3oPp3YVv2Z3XyQXFLYxUrM5Bs17nLEUJcGjkchBJAoJ` | XAU call 4000, cap 4400 | 2026-11-02 09:23:11 | `5D1EhL66mjHCdsktpqztvvQoYGWeVeABT3Am5s71hyydbShVVeNR9vvTYHbmxX7mPmTDyRHQK2pppeJD65MVNutU` |
| `6dPzg938Z98ToWt5CXYRaGYEPsQpQoZutaqicTkYs8c` | XAU call 4050, cap 4455 | 2026-11-02 09:23:22 | `2nwySSmKWyBee5MLHbcj8aSRxBNmwddo5Y37u8zwfep5D4JYqygw7Pmf3LjTk1o3xzC2w2htez8XGdfuibwTwGK6` |
| `96a8f12ubdcRpVPh2qAk4X8WLcZrRwud9dPZP6JminT3` | XAU put 3750 | 2026-11-02 09:22:28 | `3gdXjy5K8fqzyGR2akMXHHsGR4hmJVb13ktN3MGeRQYYUwdcivnXqLC7puXN4v4as7xZKp4NwrwhxY9PwXXMUbaN` |
| `FDn7RNaUcFb73vwuBv4r7uYaBssUcpu2RGeNoctYNcCe` | XAU put 3800 | 2026-11-02 09:22:40 | `2froit3RguwuG3CqozdX3zbDJCARSExrN5mGbgVShmidb7eScKbRpke1GA2Jfdvw6jX1cwvP8NiY4UF7WmtftN78` |
| `2hd2i7zss2bJRAY5BexezAjYKuMCqZPEpWwzCoNm33S6` | XAU put 3850 | 2026-11-02 09:22:43 | `5FUVAi1GMyVzj5bSAAFtEf9kWSCTnHYrLpejxJBwRqQEPaVhV4vekzvdY16YuiQ9PLhzQtvZtyE4pAzkvyzkPEXm` |
| `CBD3UHb4wgwNsngCkbDRNffh7uWFJpV6Xj7PqVA574x2` | XAU put 3900 | 2026-11-02 09:22:55 | `2ayDJ3EhtcdWRQ2B3FVbEJbtrqExWa765VW3eeWGfx8AwsDbGqsYshuXFqiuJAxtC9LPDsEto2UUZnURQtxTMhgp` |
| `AsihMyQbtFk6PBHDuCfF7HXZgfb3ZbpXejnbyYYQkiu3` | XAU put 3950 | 2026-11-02 09:23:07 | `4qAeRgPLsPkspMo3DYT3RYRG2sjZjPV3mkVuvaRAyYiZKf6xk98rGH3cTMGGb3NTYS83X4jhxn69eN6AL8WSrDSU` |
| `FeWTPxDkxqYQ4y82zB1hXYoUTv6mCtEJdxunVtLgVXy7` | XAU put 4000 | 2026-11-02 09:23:10 | `2ephggRkWuwK7z9xS6AE7Q9sPSiMr8ntdSU4vAgFNXhhCtBRVRzZpTsa7Ma5Nv98J1BNCNA3WcXGH99ueCsvm8L7` |
| `D9TK5Ah7NDX6jvcLchs91gZjHqSXt2gBDdRJx57ikZFF` | XAU put 4050 | 2026-11-02 09:23:21 | `3Vf5GpdLQWP5d66hWeVrkgBRLXgQ4mUaqQ65a14aVeBku2eVX8zurFbZFGZ2gxSxrbf1njDWfgfmzaPDaQC5CNTW` |

**Oct 30 reclaim** (2026-11-02, 09:24:49Z to 09:26:34Z): the 23 settled Oct 30 series
were closed, each returning 0.004699 SOL of rent (0.004694 net of its fee), about 0.108 SOL
in all. Their accounts no longer exist, so their results live only in the table of the
section above. No `reclaim` has run since, and none runs until the phase 2 records are
published (`docs/PLAN-phase2.md`, R6).

| Series | Closed (UTC) | Signature |
| --- | --- | --- |
| `Dr5DN1cyYkNq4Xe9cKEud7cXDPjk57ERhhzvUQdBXHA` | 09:24:49 | `3CXKs2cfdS5XKNki3vJebaRs7mG7tDjLkKj1d3ZpJhTA9irLxnTXvaZrbUbt7792xNBT1MN3JmZ8TkRoRa4q2SAz` |
| `JvKCAALDRAxd77sCLw1as17CQDXbvsg2u4ydbtGxQAi` | 09:24:51 | `65r1H7yK9SmVAhumzn42SBv5Y8rA3wNckQPppw4pmNkWEosGYjhEY9VBpjdGSxFAXFFVvzGRTiqM7U9v5LvoPMgm` |
| `2CpwYjJVt33VQ6b8NBHSiqYxJASnk2ZtWvv3gznpswU5` | 09:24:52 | `9JiJZcQ3eUqmNmUZhvvNuDaUnYBkexFust2Y19coeP1SnKijWeZpxsEe9Kd8TvBDSCRFSgpeefKe2dzU6SSBFjL` |
| `3hhKKRXq8bJne9aPH2P5vtDjnaY1neJuTRpVhzDy53Jq` | 09:24:53 | `5Ti1meKPr3vbQFMDSs2xhByvwdfAJbwjCRSXWuM1oF9VqGM3zhbwoEkvpLsgUdE3sfPTs8kgYbmFHBLGqtLqjetK` |
| `4BP9Rw5jAprEPqf476bZmZd9W5pibvRFNWKowZeKuAth` | 09:25:04 | `4mKyXALLRU16JXyh3k7N2qmUq9SGDcQHhwvWjy3RKZogTnJdj6zBp4ZGVstNXXhpXu28puftv4x14i4CgQo51gB9` |
| `4W67aaLu1gUYwBszn9Zubt1hAq2wy4SL5N7XRUczF9St` | 09:25:05 | `5tEYM34oF8Z83xaJppUPksXYR1cYS87mXYEERk7U9U3KfKrPDtAh2TFcfmR8MLxDsdV5DW4k9D6butNGRYAMPeMu` |
| `4u74R8Zn88TTxFHaDBVifQKSHgyJbsGCo9ndhG4raXhv` | 09:25:07 | `241iazQQqTqgxZz1MARaLkkfLyMcaWZ8CMYkRTHqSxCCfEnXRaoSiuRfkGMvWK2HtzecM9ji85jcR7TaucpEpiJZ` |
| `5vvy5enzqBm5zfZnUFnDs8K5AG8JEEwNbspvV2CDusbN` | 09:25:18 | `2RKtMvc8z88J55XcBg76weFdeUss18jAqbmn9F1SYcC2Va1YdxT8XFW8gZJVKNdC7y7jBDuzjyGc4knjwLrnFmsX` |
| `6ZbdqcGTddC3SoTtggHEWWyDj4XJsFGwU58GVZmktmyM` | 09:25:19 | `4ncC7dWsqXfSyVsNYJ9i4pjz4tBQ1EDJUk4RAbpq35S1M1o4WK3JiRmkZH9yyDz6NvbBAbLEuw5QWk9xZDWou1kf` |
| `6qg3wruL2TocFw9f4Q3CXA8mmWx6GKsGeMTUphPJgF5H` | 09:25:30 | `66f4nQBvVFviX6x8UpSyxn54KcGYdfZd4yDU3P5LtDMtVoiMzPnHhcqaiGYoUecfP5m4t9CyeaL9HBUXiSTxZgr4` |
| `8Lv5WwKejsFCrawBAvVvUgEdsSmCBaARevXgLzvSUCAU` | 09:25:32 | `243r3jwWSLmqDxJUV3Y9uQbN6JS2EnJ4bXs21NXerMv5jzp4sVauSRRSjKKaKRUEMRK83RZkGGDuax5qaLqEYzzo` |
| `92xnofbYSfeSqvoxrdNoPfeU9kpfr7FDu8JeijgasQiw` | 09:25:33 | `4suEDcomsmZguFqRJryHLM8upSLCBGNwhLz48zxvDVVxv9JkUyQEJjqzAVyAgse8yTuA5GYwwadi4D52kqa8LN3U` |
| `9HGpi5WbLxWSZfU3WuydbXMPfNvJrdA5T4FC3Dtee57W` | 09:25:44 | `ttvCfokpxPr6ixbQAvmbc8JQVffmZXctzTBfvJzticjeNkfDUZGmxmcjWEQAbqVNRQ7aJE7wvqZvseybvV3Vwhg` |
| `ANtq1DXY6rg92dag8sLHLzm8dMG5t2Pn6CZzZw145oon` | 09:25:46 | `3TUaXXG4TwSv67BsMTGnKYY6E2uJY5GPbLNKPYaASwq2BDEcSwMNaW8terxvvsh5cwgK5RMve1JVRdj2k8gYkNSV` |
| `9d1C6cKzUMCTh7Uj5jLQmF1FNLBpbmX5xfsgTQVJRVLK` | 09:25:46 | `5CEiU16CJ4rz324m8ia4Kvmzu31hMEiU6E4yjLqZvv18YSeDx6VbUcTfmdv7GbbFzCcr3oo3FNeGyYpC2ki9xaZp` |
| `AqnB9PpgUAhW4tmqzt2Srzsgdfm2BzU6m2jbbfYYpBi4` | 09:25:57 | `2rddTNTNs6oFsMb5nQSU3N8pNop6au4Zh2GS4wDodmuxnSLCVCqXJhv8q6cEoeFS3hcxTAXGXooGbE1Nsw6q8ZSg` |
| `B5pPdQXhGKnEyWkU2PZZzPV1kio3jaEoQGFBSh4D4uoG` | 09:25:58 | `27RSSEYGj53gme7JNP1S9jNcsEdPHKtQxBhpda3S5m6Tv1VJG7Jz4cJVUfU46mbm3EGfWUMkTiMkddAWf3J4rBoz` |
| `BSpa2zeovzEUdd9dZ6LwfNuw3BpBVWpetna9qYDqYG4R` | 09:25:59 | `3MaMk2pk8zR5BM67L5PAUaLywPFLaZTUD4whLCBNow7bUSRTGQd8ug2rzv3JWQT7ykAd6QbuqFJUaVXFA4eukC62` |
| `BvhmabWKVnDWacexfQw58U1uGbU3HLDV4AfHKquuaBbi` | 09:26:01 | `4yM4QTWxWwnb2ENcDfSUCAXiS79RG8CebQqxMJsvX98km7gwuhTyMFLB2z7jjpy3qW6sTLPdbCFUoomWHazsPBYg` |
| `C5CA4egx9AWTVg35Y3k1EhkwfcYMGdGs5hNWDrVM6UTX` | 09:26:02 | `4Z3BQDcuuFXdm4p8yyhBZPceGjLQsfgjbt5ZxJLJnTnFwHDwGkToWM71dTr77QgjhyRjmEKwypj5nusMZVd28o9Y` |
| `Dw3skzBfb9S3kBrRnAvSkvGKoZcRbFy178yZ93WSiL1y` | 09:26:12 | `5VCaUTdiLtC3GvoktwhHxew9ptdP2Rk9M6nXRzQHr5a6ZruPFVjah5gP4hiLWVcJMgzskoF7eY8DFMj3oPyF8iX2` |
| `EfyCdzYUmTY8ie6XtbHpQAgzA1bBZdhC47wK6Ez6JyRU` | 09:26:22 | `4WpzrP8XLGKAT9amBp2eEyiPDD68CTfjfEgtXdaK9iFKPHpnqh39rVCnShXFZ6TAS1ASeRu4XciBTSUQ7KTjzT5t` |
| `Hs2SMW7Uv5TkPzBpZbLmPNuGepAyrUhmh1FfU6Yxtp9U` | 09:26:34 | `2U4WA5v66SEtugQzpBHUxLg7vPfeDJMVRgtXHSxL5W1KWz6EZueuBo1BrFb4Ue3czc5WrfJhND8z7wtV5f3iLkth` |

## Key separation (H3), 2026-11-07

`docs/keys.md` (Devnet separation), steps 1 to 5. Four new keys under `~/.config/solana/`
(`solana-keygen new --silent`, mode 0600), funded from `77ie…CPPV`, then the two authorities
moved off it. Every transaction signed with `77ie…` was announced on the bus first (bus 60630).

| Step | What | Signature | Time (UTC) |
| --- | --- | --- | --- |
| 1 | 0.05 SOL to the upgrade key `74jZtwqQ1n15ggR4rS3K94a2gZMRtochfPceYHVPF6sJ` | `2fq1sKEvetNMfVXoWbAjwMEBXcimwADjmzsoYgw3ndJ4NjGGuVffR13vjznyZ9FtQ5Lxk6shHb14hSpB6teBeAzG` | 09:05:24 |
| 1 | 0.05 SOL to the admin key `5fvjUpFU7faq8rFRcSrPrPb6f6qRGhJzJcWZJk2As7Tx` | `2TegJBvTcWSG7wkDhAtgXDdXqFPNR9yAeDcnTWPmubmcXuG73GfTufY1pMCA5mvz7Tsha1JGWaHPyxcsamgBq7o` | 09:05:27 |
| 1 | 2 SOL to the keeper key `DuxGX5znvbiHFHk4rpKmP238zXf5eXTK4xNLN44ecSY2` | `2KbE8vKDaGEeCJ83DT2bvQgqqDKMj8wMHXhGzPJDNQqgxbvXDKKEdmmMu8qmddncPAYy8YurkSe8T3xugKAiDKsZ` | 09:05:30 |
| 1 | 1 SOL to the lister key `QALJUrTtz1GJDT8oiW9V4DRfPW1BZzFJjQ7kbd9zND8` | `nMSwpjxNNh2bWAH5HjVUeLy82g5rmRGSGo69RoihQrauj7HMs3XqEAiKdz2uk6tEwQi1Uvv8joyWonHkW7vC7xk` | 09:05:44 |
| 2 | `set-upgrade-authority`: `77ie…` to `74jZ…`, the new key co-signing | `28ZH5RErqUcL1EvFz6pW69gBgcgsjne81iRtoXtSJTZEkfYH7eJEHyXM8B1Ka8iWN9gcQgoCYX2oy4MDL2GCtoX1` | 09:06:08 |
| 3 | `set_config`: admin `77ie…` to `5fvj…`, parameters as read from `show-config` | `389v2nge9nVmGaTvGxKpc9nLFUNSeTVA2rTVuXkFLHBupMTrL3ojVzY2e1f9Tq7vedTy6j8aDzJVuhAsazQ4fpKS` | 09:06:46 |

Check (step 5): `solana program show` names `74jZ…` as the authority; `show-config` names
`5fvj…` as the admin, and its `params`, `usdc_mint` and `address` are identical to the read
before the change. The keeper and lister keys appear in neither. Series listed before today
keep `77ie…` as their rent payer.

## Demo maker funding (phase 2, R2), 2026-11-07

For quotebook's D5 (`docs/PLAN-phase2.md`, R2; bus 60617): the maker `7tVP…cACQ` sent 14.00
USDC from its USDC account `CTMwGLgvpCBY3oZWaWUrWCjMfkn5VgeMGc5LbhjFL95Q` (0.23 left) to the
demo maker's USDC account `F7KjgYNYkFK1RLF81yZcfRKq1wEb9wCDDSSrVUPe8Co2` (owner
`142GM9zKDbdncitgiL6zwdKquwuzzNhMPXwEm1RVYaVB`), the maker paying the fee: signature
`uQAmw5xnP8Atc4aA29HS3bsF8XXmBemnENznW2gXsvBWCWrAaXYaY6q4EGvQ5r4tWFTmAFdJkGnAkW5zRDZxnvW`
(09:04:31Z, slot 519608843).

## Weekly series roller (phase 2, R1), 2026-11-09

LaunchAgent `com.metalbook.roll.devnet` (6 h interval, `--min-expiry 1795208400`) listed the
Nov 20 21:00Z series (expiry `1795208400`) with the lister key `QALJUrTtz1GJDT8oiW9V4DRfPW1BZzFJjQ7kbd9zND8`.

| Run (UTC) | Metal | Series | Lister balance after (lamports) | Net cost (lamports) |
| --- | --- | --- | --- | --- |
| 00:39:58 to 00:41:22 | XAU | 18 | 896,125,600 | 103,874,400 |
| 01:04:34 to 01:05:41 | XAG | 10 | 838,417,600 | 57,708,000 |
| **Total** | | **28** | | **161,582,400** (0.1616 SOL) |

The lister started at 1,000,000,000 lamports. Each series cost 5,770,800 lamports. The 00:39
run listed XAU, then refused XAG (`published 83 s in the future`) because the clock was read
once per run; fixed in #60, and the 01:04 run listed XAG.

| Metal | Type | Strike | Series | Signature |
| --- | --- | --- | --- | --- |
| XAU | call | 4100 | `6t5yHgG2iysCvidomhpnTHDEKApYdhAWpY6NDGPN8cZg` | `5UVh8UX2CpDVLs2tEtp5BA4urMoSLGge8FMdgpU7UBVahVCL3C6PLKkqzGQvi11WRAc4SmESm5noCQTYAcVV2BXe` |
| XAU | put | 4100 | `C6XNXmWB8TzixuFdccca4mSixKsyyL8MiAMUTci5ZKFH` | `5MwF95tWfu9TGJfKkFYUBHtVNmnbDPyLqn7tbsDeAkQkf2wCxJbYY7fX2swvTdRhFoRmx21pLKRkCEhXLkGiJsmw` |
| XAU | call | 4050 | `DuEsSb5FhvSyAK6b8N4FXYatC2pR9MdUURPmYHwdXz5N` | `GYJe3LgrQtTVbmtamw48BMJxVeJWmzkxEzJt36yrxFcm3vnLSe2FiqdQaEs2WFn9LZCoUPMvax4zU532cQnh9Yq` |
| XAU | put | 4050 | `FCpUpz2AZ3GLyhCxjxThr82BcfixtDsuMw31tKYhUtbs` | `61br3HLLn1GHAzr2e4js6rToXyDAyNtRH8MUG6nnW8g1inix2JdqhnMT8xkxT2SssAT8qTnRtGjK1WdsTYrF544C` |
| XAU | call | 4150 | `F2Z4uhmt1x7ZJStEiVKeJ1CT3NoBThd8j8uNLeHu1UhS` | `5TEVEHEu1sZQhBk69v6WBzJ6JnVzk8PgCMm77EyCZdLtxaezzkjU2FJRVyXA9VQxNeuR3qXLevj9jS9rgm9bzxk3` |
| XAU | put | 4150 | `9QtY9dzYi5uKuwMgD4RNFm2GkeYWtwFP64NZdTPvETWu` | `2wmGrLihpBnK4p4HZV8uEVDx1XGYr2cxwiDKFXs1tyJmtNEAH3sYB3yEqguu7FHoTJ7KTXANh3PM95HrP4kzbDX5` |
| XAU | call | 4000 | `7o6j1bXazSkziCpyDBysAHcefhZaRyYiWsCUDex1UY3Q` | `vDCWhaKcMoyS5ULrD2Xon2snmroDqgsWRkCEBJ5pnzDxrrJDdV3FeHQ7bYNxkgD73ofqK4d58sPshr3nvtsBW3n` |
| XAU | put | 4000 | `ChHEBkdCeCpGNsEkT4JmiBuPadm2M9pm5YDbBsgFojNi` | `4GHwe9893gHGBwcF2UKLJ1DLekyRaECzxuvPRCAwVzJSmmpV3BtgH9kUhPhydizXRQ63xgvoHSH2EbZZtf3g5SYj` |
| XAU | call | 4200 | `2JF4VVNttuFhBF6aC5enVzPxSby35qy8DvtEisCnuPs4` | `UFKFmEYEw9KmzMXN3UjFU8qxaXiJMr6mvDck3Bz5eLrhV2tK7NaHfx8wEPEKi7WGP7hJtPysiEZq2vNXyGLF54D` |
| XAU | put | 4200 | `8GvYQABgDhSLpLsotEaEv6oZqthdimrKAmF9BzJjxVu8` | `aUiEPqrnfwJXuY7Jj4UW73zbDWwkyr9xt1GmB3HtA7z7qokY9Ug2UAgYX4efcPWaoJ7Bgf9rYwC8inA1duPi3o7` |
| XAU | call | 3950 | `6uGxMu58fed5vczs5zub1VDGKSihbyBEaSGdNPUuab3p` | `5fmPouXyAoggx3scuv8PkrzcXvoEJL65HCmKrgnqmNPBt1S4nkF6zhb7CYLES4RUce479tYUpYt8BWqg112EabH8` |
| XAU | put | 3950 | `Bq8VjFhebbXpANfAVr1MKyroexnfPZ5P7XhHkXSCsVme` | `wacfxPtQLfvFSkandUR53wvpvaNGD8SqL2WWshJngTh961PUbVehegkN439iaGkj2JC2LXKiKn5qTwiNrQesMqJ` |
| XAU | call | 4250 | `7jHt1r3Rb5tZujyuWZYgWxQTmNUZSVB7Utef7A5d6NBb` | `5R4m2eFAV4QHGdmbPx47xLJy1HKg81eSWoChUMkRE6Lxy9FmQEig87xMZTcFKGmhk7sYqNBS2ecPVfCjGrR1w8uF` |
| XAU | put | 4250 | `5TbU5aFAKr2zGQDPucYyUBhj1rZgLv9cGrcC1DJrF5ve` | `4AVYgnRmuQEg52sFTs1qKXzVD7Zr9LwCh6HYrp4NyvsvzvbtdUkN7unjAmYhR7U2wEraZjDDb8b3fvBYTWh1qZNA` |
| XAU | call | 3900 | `E348M4NFvTF2FP7vFCAaHgy5ZPurAwS6a2tF7JX5Lx7g` | `2XjML3vCWKdtUYc76t7Kx9GeGV4bhzq6JqoeZVJLW9sjo14GXiZJb7VphveVyDYQvx37KBBSGyHtnFDvvn8ChxdJ` |
| XAU | put | 3900 | `BV94EXotAMonBUxzYdw75cE7SCF6GSF2szQeRL5Ftygk` | `2sirsSpjTfLRdTiExJZzRasY2ZvSDXchWwubBwzrTarwFzxroWmQmbZS4XGE8e81VMSoNyuFxj3SZEq6rXUqRfJv` |
| XAU | call | 4300 | `7LA7fC8rGB8s3Zstw3p1xt4iyFCwmnAwhWNbx7TeCw7i` | `5dWenUWsxC2GejFqHm5Xa7eFHFQsk8aG6T2aravfaTAaZabDFhbvEA51NXyAdCTsowpDfYAjWuEpZoNikcvaMMS5` |
| XAU | put | 4300 | `9ctNZna8nrTBqnEQwzbaWkVQWAEUAnuZTunYNaKB28hY` | `4rxZ5AP4YG2cddFLdVBgwDhv9UgWSHw6pb1MU5w5Cyt6cewtTQoXCziGMZmUEsmXhwAtLGop2xvXEsKZxqFuxUAU` |
| XAG | call | 53 | `EugHrksN1GfKjaYiKdwX71ErgSibRHYTFUNk4E4Rd6t6` | `4zH5qkkWgR8r6mYEG9uo14hQ6Qevbghzaw9nwbEHYENXvEb6HR27sCBTpB7EkfhZ5BgaGSjfAa7vTUoyEQ7kBwUB` |
| XAG | put | 53 | `E3K7AbTG5ZDEytBkqYEkNKDFFd5rQUQohUofVTWdzqQ7` | `2drZhM7vB2BbJeNLUh3YBLxdqcRVT6NsCLMsz3Ksj42aKJNQVq2N3UzrX6UGrg2st1HJreP3pZeJpLzAWSV6TnTk` |
| XAG | call | 52 | `ArCjPYXJDda7a7vDqKBzU31nU6eyNzWYdJb4vLHKZrzH` | `3VpEGDCBykgVT4miymPZrDPjvH6MoGAYCSshCXSDRK1TmuuSEahgZsHC5Kmqu3vUGmpZ89tvMYw6skE59M9qNpYY` |
| XAG | put | 52 | `HWQpYHXB4EDGW1wUzKbJ2YNbqhoiAJZaHKqiyiSv5iQv` | `2bHBGuv5RiQb8KA2kjTc4yHhBwCx89N4z9fSecutmXDixpuzgidD6y91BiJG46SwGKRnVRYNg5NMAGJexP5a6cfk` |
| XAG | call | 54 | `ATrKvXT88usF45TSJji9bP8LWtqvvChKdzp5GyfGzLBw` | `2Cwfsn4hdTMTNbmVLdyw1hajY9TGoMZJGM7cUTMkoxngDCiHfy6NEntDeyTDHoEbUCk7KUjjmmoCRyVHP3cz2DUU` |
| XAG | put | 54 | `Avus1HS2Veotj3cv65m33XV5ggvXddPneXyraiAEB4Kh` | `5s7M5FEdfTQTasCfiJpoLcpwMVHKg9TF3w9kq1Gc3PDgPhrgxTUsBvt9DY3vmYjfYgRw4xzSK7B8bsS9jtPFizvv` |
| XAG | call | 51 | `GAN7xJY69zhH6Dr6VckPYffo3m7HALZnLfpaFyvAGVbr` | `4VEAbEvQBBo6etVdzo132axPizYsujHfj3CBXDs4pSo2iDaXNmDAosH4H7Lmobf8Qp6G3W5HDwsQsrJWwcaG47iX` |
| XAG | put | 51 | `GH1FfV1dh4h5MwcChc48X6nuh8CcbBB3XhjTKV8vNB95` | `4CJ6CMxf1x6mwouD458DtvFdYsw5fYPLqXYxaB3g89KBJy5FhDv5XjBoLxyXJhEzmz7oBvasymxQxuMSwx4JXr4U` |
| XAG | call | 55 | `9StNKdcJ2X4rJPEQR7Mi8cMAAVpEX8fgXcSbQ92mTJot` | `2WsYDTZTMBJsH9bfwAgQCxcSf7XRkZwaRRwbRhsurBDbmgv7NbYP3VimrvhhqSwzbrPTs5kV7HzRFTT2K34UAyH9` |
| XAG | put | 55 | `BBKAVGnqV9KLnW4cMhW4ntqgoAtQY5DdCa1wZoCwz79d` | `WbaksG6PuGhJpDu8KwoQyKQq7xruGNYssEqEpP5Ztq7F5xWHCF9dkWxGKkFJQsZRXReAWUkNRGKJfKTQHZa62MA` |

## Not done yet

- **Trading** through QuoteBook on devnet, once the maker has more USDC.
