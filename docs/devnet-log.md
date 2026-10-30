# Devnet deployment log

The first devnet deployment (milestone 7), run on 2026-10-30 from the runbook in
[`devnet.md`](devnet.md). Every address and signature can be checked on devnet
(`https://explorer.solana.com/tx/<SIGNATURE>?cluster=devnet`).

## Program

| Item | Value |
| --- | --- |
| Program id | `FPPuou5kgSp6tbwQRwKPBNhrMyf1Xb8n2ib3Yqpbsuwg` |
| ProgramData | `DSptVxC9DvwxqEK3qY4hTUuHb8K6e6wEbWi9r18XMRVF` |
| Upgrade authority, admin and payer | `77ieYLKspRnGzAt1AK6ZVsttnTY6T7EY2k7CpTZeCPPV` |
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
50 oz XAG series got no posts and are left to void after `hard_deadline_s` (7 days).

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

## Not done yet

- **The void path**: the ten 50 oz XAG Oct 30 series reach `hard_deadline_s` on Nov 6; their
  `settle` (void) is to be recorded here.
- **Trading** through QuoteBook on devnet, once the maker has more USDC.
