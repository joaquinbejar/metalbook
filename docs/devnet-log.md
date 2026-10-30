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

## Not done yet

- **Writing and trading**: the payer holds no devnet USDC and Circle's devnet faucet is a web
  form, so writers cannot mint yet. The maker needs USDC at
  `7tVPWX3zmxYCZ5ioTSX4yzdNq8o3Xr1hMSv2kz12cACQ`.
- **Settlement**: Hermes answers 401 without a key on every price endpoint (`hermes`,
  `hermes-beta` and `benchmarks`, checked 2026-10-30), so the keeper cannot read prints and
  M7b-2 cannot record guardian-signed updates. Series reaching expiry without posts void after
  `hard_deadline_s` (7 days), returning collateral to writers.
