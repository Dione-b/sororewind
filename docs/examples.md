# Offline replay examples

Run these commands from the repository root with `sororewind` on your PATH.
Each fixture contains the recorded RPC responses, original transaction hash
and network information needed for the replay. The commands work without
network access.

The examples show the default text output captured by the narrative tests.
The exit status is shown separately because the CLI reports it to the shell.

## contract-error

A token transfer fails with `BalanceError` because the resulting balance would be negative.

Original network `testnet` and transaction `dd1f9bb61fc8d599aa7914469f4e013b40648a6dcd1adb6d294467f55efa53db`.

```bash
sororewind --fixture fixtures/contract-error
```

Expected output (exit status `0`).

```text
sororewind  dd1f9b…53db  testnet
ledger 5672381 · protocol 30 · 2026-11-11 08:25:48 UTC

findings
  error      the transaction failed: contract error #10 in transfer

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  contract error #10 · BalanceError
  "resulting balance is not within the allowed range [10000000, -15000000100, 9223372036854775807]"
  in CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer (depth 0)

call tree
CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer(GB2YUAICRUQNAHAEAYDA7SS4SGQTECKKCU6PQ7BX32ZRX67VBH7MSCWC, GBZH7YHAQYRG3MQFKFZDOGIKEEYGZ6LJNCEHYOA7K7BDLJGLQI6R3RAG, 20000000000)  ✗
```

## nested-contract-error

A nested contract call fails with the contract-defined error `Limit`. The call tree and failure depth identify where it was raised.

Original network `testnet` and transaction `4420c14ada7c0414e83a018af29d3605a0e1c7317f01c92629ba8874b2a903a0`.

```bash
sororewind --fixture fixtures/nested-contract-error
```

Expected output (exit status `0`).

```text
sororewind  4420c1…03a0  testnet
ledger 5678938 · protocol 30 · 2026-11-11 17:32:13 UTC

findings
  error      the transaction failed: contract error #17 in execute

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  contract error #17 · Limit
  "failing with contract error [17]"
  in CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO::execute (depth 0)

call tree
CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO::execute({actions: [[Supply, CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU, 300000000]], deadline: 1794418443, epoch: 1794412778, nonce: 1, target: 0xddf6020d009b0ffe…(32 bytes), version: 1})  ✗
    · VM call trapped with HostError [execute, contract error #17]
├─ CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCOF6WEYQQUFIMR4SEVELW4AGZAOYMGZCYCQVXN7AFGWZXXVEDPXQ5K7::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_positions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve(CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve_emissions(7)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve_emissions(6)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_user_emissions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, 6)
├─ CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_positions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve(CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::submit(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, [{address: CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU, amount: 300000000, request_type: 0}])
│  └─ CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU::transfer(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF, 300000000)
├─ CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_positions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCOF6WEYQQUFIMR4SEVELW4AGZAOYMGZCYCQVXN7AFGWZXXVEDPXQ5K7::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::balance(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_positions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve(CAQCFVLOBK5GIULPNZRGATJJMIZL5BSP7X5YJVMGCPTUEPFM4AVSRCJU)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve_emissions(7)
├─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_reserve_emissions(6)
└─ CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF::get_user_emissions(CCUHQJ3FSGC26UOCLN3EK2DUO725ZLBVRUF5B7N36W6MB3DM4JEOC6VO, 6)
```

## auth-error

A token transfer fails because the invocation does not satisfy the required authorization.

Original network `testnet` and transaction `b5f65dde0461c2975ed47a1b6f8f270c0de5a5918daaa2fa4d8c4e44670c00a0`.

```bash
sororewind --fixture fixtures/auth-error
```

Expected output (exit status `0`).

```text
sororewind  b5f65d…00a0  testnet
ledger 5670652 · protocol 30 · 2026-11-11 06:01:43 UTC

findings
  error      the transaction failed: auth error InvalidAction in transfer

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  auth error InvalidAction
  "failed account authentication with error [CDMBYMTH4BAE3CEFR6XNKCMZD6D7JGKOTKJW742KMOMQH5ASHRIWLAPX, contract error #3]"
  in CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer (depth 0)
  caused by contract error #3 · InsufficientSignatures
  "escalating Ok(ScErrorType::Contract) frame-exit to Err"
  in CDMBYMTH4BAE3CEFR6XNKCMZD6D7JGKOTKJW742KMOMQH5ASHRIWLAPX::__check_auth (depth 1)

call tree
CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer(CDMBYMTH4BAE3CEFR6XNKCMZD6D7JGKOTKJW742KMOMQH5ASHRIWLAPX, GD4DZC7H3D4YCJOETIMPKVX2Z67K3HMGZKKIIUTCFLXOYS44LUYUYF66, 10000000)  (no return)
└─ CDMBYMTH4BAE3CEFR6XNKCMZD6D7JGKOTKJW742KMOMQH5ASHRIWLAPX::__check_auth(0x856c8c546bb9759c…(32 bytes), void, [[Contract, {args: [CDMBYMTH4BAE3CEFR6XNKCMZD6D7JGKOTKJW742KMOMQH5ASHRIWLAPX, GD4DZC7H3D4YCJOETIMPKVX2Z67K3HMGZKKIIUTCFLXOYS44LUYUYF66, 10000000], contract: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC, fn_name: transfer}]])  ✗
```

## auth-check-auth

A custom account rejects authorization in `__check_auth` with `SpendingLimitExceeded`. The report shows that error as the cause of the outer authorization failure.

Original network `testnet` and transaction `9340ee7cce6d7d0782ccdcea7e97f61c4d4aa5173166fe086c239e07672e5a96`.

```bash
sororewind --fixture fixtures/auth-check-auth
```

Expected output (exit status `0`).

```text
sororewind  9340ee…5a96  testnet
ledger 5549745 · protocol 30 · 2026-11-04 06:06:05 UTC

findings
  error      the transaction failed: auth error InvalidAction in transfer

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  auth error InvalidAction
  "failed account authentication with error [CAAHLKA4A5IXI6WXVIL65F5P7ORPSFWY74EZMKHRK3V6YY23L75QSI7B, contract error #3]"
  in CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer (depth 0)
  caused by contract error #3 · SpendingLimitExceeded
  "escalating Ok(ScErrorType::Contract) frame-exit to Err"
  in CAAHLKA4A5IXI6WXVIL65F5P7ORPSFWY74EZMKHRK3V6YY23L75QSI7B::__check_auth (depth 1)

call tree
CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC::transfer(CAAHLKA4A5IXI6WXVIL65F5P7ORPSFWY74EZMKHRK3V6YY23L75QSI7B, GDHBFWHBW34PIFJXB2M6AUA7ZMSULXVXRRWKIYTRLC4KFYKTMKOZR6R2, 10000000)  (no return)
└─ CAAHLKA4A5IXI6WXVIL65F5P7ORPSFWY74EZMKHRK3V6YY23L75QSI7B::__check_auth(0x10b75316aa524fc5…(32 bytes), void, [[Contract, {args: [CAAHLKA4A5IXI6WXVIL65F5P7ORPSFWY74EZMKHRK3V6YY23L75QSI7B, GDHBFWHBW34PIFJXB2M6AUA7ZMSULXVXRRWKIYTRLC4KFYKTMKOZR6R2, 10000000], contract: CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC, fn_name: transfer}]])  ✗
```

## budget-error

The host traps after the transaction exceeds its resource limit.

Original network `testnet` and transaction `608924e5d6fe4111acf39ead2605bbdcfcdf1e06c3b288e11595fd0b95eec093`.

```bash
sororewind --fixture fixtures/budget-error
```

Expected output (exit status `0`).

```text
sororewind  608924…c093  testnet
ledger 5670060 · protocol 30 · 2026-11-11 05:12:23 UTC

findings
  error      the transaction failed: budget error ExceededLimit in __check_auth

✓ replay matches network
  on-chain  resource limit exceeded
  local     resource limit exceeded

where it broke
  budget error ExceededLimit
  "escalating error to VM trap from failed host function call: verify_sig_ed25519"
  in CCQ2VQX3QXE6OXXTVN77XTLKMWFUKW7HJAENIPEELVRIU7JSI2TT47BD::__check_auth (depth 1)

call tree
CBELZBFWB34SBDC6BUXD5MFG7KQBVC4L6K6HE7QJBPL72L3OTR4RO5XK::set_authorized_router(CCQ2VQX3QXE6OXXTVN77XTLKMWFUKW7HJAENIPEELVRIU7JSI2TT47BD, CC3B4UP4AXCW4IMJCDXIT3Y7CFW7QFTCJXPSHYNKLQEDFHZA76EFEWGS)  (no return)
└─ CCQ2VQX3QXE6OXXTVN77XTLKMWFUKW7HJAENIPEELVRIU7JSI2TT47BD::__check_auth(0x6f438fc9a7f23359…(32 bytes), [{public_key: 0x891e7ed9bb78020d…(32 bytes), signature: 0x5fb904e09992b298…(64 bytes)}], [[Contract, {args: [CCQ2VQX3QXE6OXXTVN77XTLKMWFUKW7HJAENIPEELVRIU7JSI2TT47BD, CC3B4UP4AXCW4IMJCDXIT3Y7CFW7QFTCJXPSHYNKLQEDFHZA76EFEWGS], contract: CBELZBFWB34SBDC6BUXD5MFG7KQBVC4L6K6HE7QJBPL72L3OTR4RO5XK, fn_name: set_authorized_router}]])  ✗
```

## storage-error

A withdrawal tries to access a contract data key outside the declared footprint.

Original network `testnet` and transaction `974594c7b6fd58071d6b589a84ebc7351eae3c22cacceca8b88c1c2c431ceeae`.

```bash
sororewind --fixture fixtures/storage-error
```

Expected output (exit status `0`).

```text
sororewind  974594…eeae  testnet
ledger 5672305 · protocol 30 · 2026-11-11 08:19:28 UTC

findings
  error      the transaction failed: storage error ExceededLimit in withdraw_position

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  storage error ExceededLimit
  "trying to access contract data key outside of the footprint [CBLLSKUH6N6HGFV5MSAMMJWJFYNUFEH7EGH2T4HJKOVMIZNJ23BZSMCM, [UserRewardState, GAEGW4O6ZSHPSVPJPT3WW4IBY2KKSCNDMORU7N5PK54WMRACVX7JD4BM]]"
  in CBLLSKUH6N6HGFV5MSAMMJWJFYNUFEH7EGH2T4HJKOVMIZNJ23BZSMCM::withdraw_position (depth 0)

call tree
CBLLSKUH6N6HGFV5MSAMMJWJFYNUFEH7EGH2T4HJKOVMIZNJ23BZSMCM::withdraw_position(GAEGW4O6ZSHPSVPJPT3WW4IBY2KKSCNDMORU7N5PK54WMRACVX7JD4BM, 59720, 60140, 854358050, [403326, 177772126])  ✗
    · VM call trapped with HostError [withdraw_position, storage error ExceededLimit]
```

## refundable-fee

The host completes the invocation, then stellar-core rejects it because the refundable fee cannot cover storage rent. The replay also reports a rent-related setting that changed since the transaction.

Original network `testnet` and transaction `8a5229c1e8b7582ba6384f830216b4711a837c7b7e13d1b6700aedb3398011e0`.

```bash
sororewind --fixture fixtures/refundable-fee
```

Expected output (exit status `0`).

```text
sororewind  8a5229…11e0  testnet
ledger 5678910 · protocol 30 · 2026-11-11 17:29:53 UTC

findings
  error      the transaction failed: budget error ExceededLimit
  warning    network setting changed after the transaction: LiveSorobanStateSizeWindow

✓ replay matches network
  on-chain  insufficient refundable fee
  local     insufficient refundable fee
⚠ the match rests on state that could not be verified:
  · LiveSorobanStateSizeWindow — network setting changed after the transaction; the replay used its current value
⚠ network settings changed since this transaction ran: LiveSorobanStateSizeWindow

where it broke
  budget error ExceededLimit
  "refundable resource fee was not sufficient to cover the ledger storage rent: {} > {} [8992, 5119]"

call tree
CBDVQKYEN6QMRGQZC77DFYEQXQHDMCVJ3TPBJKNERJVMIESA6GQT44LG::relay_pyth(0xe4bd474d4e2bd8d0…(295 bytes))
└─ CAYFT5JE3UQTKT4Q6ZOZK4FXVYVT6RE3MFC7STA4UB6WAEGBT65MRU52::verify_update(0xe4bd474d4e2bd8d0…(295 bytes))
```

## rebuilt-entry

An oracle update fails with a footprint error. The replay reconstructs the earlier value of a ledger entry from recorded ledger metadata. Add `-v` to see the rebuilt entry in the footprint.

Original network `testnet` and transaction `ce4e874b58dd7d30d6f861e8fccbace5065cc6d4072fe2badc68feeaf705ce44`.

```bash
sororewind --fixture fixtures/rebuilt-entry
```

Expected output (exit status `0`).

```text
sororewind  ce4e87…ce44  testnet
ledger 5679032 · protocol 30 · 2026-11-11 17:40:03 UTC

findings
  error      the transaction failed: storage error ExceededLimit in write_prices

✓ replay matches network
  on-chain  trapped
  local     trapped

where it broke
  storage error ExceededLimit
  "trying to access contract data key outside of the footprint [CD4KFB23CQLJ47RIPESVBTQ444R75M6QVEZULWXHHFYYZT7SS2VGXOPW, \"BTC\"]"
  in CD4KFB23CQLJ47RIPESVBTQ444R75M6QVEZULWXHHFYYZT7SS2VGXOPW::write_prices (depth 0)

call tree
CD4KFB23CQLJ47RIPESVBTQ444R75M6QVEZULWXHHFYYZT7SS2VGXOPW::write_prices(GBV54BUAQBFAS6WIRKGURXXCMXWALH4OYBCYE75VBHROJLEKJ4LOZTFB, ["BTC"], 0x4254430000000000…(335 bytes))  ✗
    · VM call trapped with HostError [write_prices, storage error ExceededLimit]
```

## mainnet-auth-error

An authorization failure recorded on mainnet at protocol 29 exercises the older embedded host.

Original network `mainnet` and transaction `75530fdcfa3d273b7e6967655b7827e302ded30cdafcd2b5fcd69b4b22eb2fc9`.

```bash
sororewind --fixture fixtures/mainnet-auth-error
```

Expected output (exit status `0`).

```text
sororewind  75530f…2fc9  mainnet
ledger 65424472 · protocol 29 · 2026-11-11 14:46:42 UTC

findings
  error      the transaction failed: auth error ExistingValue in swap
  warning    changed after the transaction and could not be rebuilt: account GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR, trustline GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR USDC:G…KZVN

✓ replay matches network
  on-chain  trapped
  local     trapped
⚠ the match rests on state that could not be verified:
  · account GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR — changed after the transaction; the replay used its current value (not rebuilt: their history goes back more than 24 changes from the current state)
  · trustline GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR USDC:G…KZVN — changed after the transaction; the replay used its current value (not rebuilt: their history goes back more than 24 changes from the current state)

where it broke
  auth error ExistingValue
  "nonce already exists for address [GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR]"
  in CBBMQBNHB2FYVZYV7VNHOJHUMTFJLR4PUMRVQYNW6RHIKZO2NQMIBUCV::swap (depth 0)

call tree
CBBMQBNHB2FYVZYV7VNHOJHUMTFJLR4PUMRVQYNW6RHIKZO2NQMIBUCV::swap(GBOT34TBVIIOXU3YIWU4ZA7H44EIXTUCHMNB6J5QMRKSU3M4R3HCDPLR, 0, 1, 64218578144, 9573121722)  ✗
    · VM call trapped with HostError [swap, auth error ExistingValue]
```
