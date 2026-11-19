# Compatibility

This matrix describes sororewind 0.2.0. The exact version and supported
protocols of a particular binary are also printed by `sororewind --version`.

## Replay compatibility

| Area | Supported values | Notes |
|---|---|---|
| Soroban protocols | 29 (`soroban-env-host` 29.0.2); 30 (`soroban-env-host` 30.0.0) | Host dependencies are pinned exactly. The binary embeds the previous and current protocol hosts. |
| Networks | Testnet, mainnet, futurenet, local/custom RPC | The `--network` option selects a default endpoint; the passphrase reported by RPC identifies the network. Custom RPC endpoints are supported with `--rpc-url`. |
| Testnet default RPC | `https://soroban-testnet.stellar.org:443` | SDF public RPC. Historical ledger availability depends on the endpoint's retention. |
| Mainnet default RPC | `https://mainnet.sorobanrpc.com` | Third-party endpoint. Historical ledger availability depends on the provider's retention and service policy. |
| Futurenet default RPC | `https://rpc-futurenet.stellar.org:443` | SDF public RPC. Futurenet is a preview network and may reset; historical data availability is not guaranteed. |
| Local default RPC | `http://localhost:8000/rpc` | Assumes a local quickstart or standalone RPC. |
| Rust | MSRV 1.89; development toolchain pinned in `rust-toolchain.toml` | `cargo install --locked` is recommended to preserve the tested host dependency versions. |

Sororewind needs the transaction, ledger, footprint state, and network settings
to remain available from the selected RPC. Providers may prune historical
data, so an endpoint can support the network but still be unable to replay an
older transaction. In that case, use a provider with sufficient archival
retention or an offline fixture.

The source of truth for host mappings is `src/host/mod.rs` and `Cargo.toml`;
default networks and endpoints are defined in `src/network.rs`.
