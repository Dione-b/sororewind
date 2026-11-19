//! Keep the published compatibility matrix aligned with the compiled hosts,
//! default networks, and Cargo's declared MSRV.

#[test]
fn matrix_matches_embedded_hosts_and_declared_msrv() {
    let matrix = include_str!("../docs/compatibility.md");
    for protocol in sororewind::host::protocols() {
        let host = sororewind::host::for_protocol(protocol)
            .unwrap_or_else(|| panic!("protocol {protocol} is listed but has no host"));
        let protocol_row = format!("{protocol} (`soroban-env-host` {}", host.version);
        assert!(
            matrix.contains(&protocol_row),
            "update docs/compatibility.md to match embedded host: {protocol_row}"
        );
    }

    let msrv = env!("CARGO_PKG_RUST_VERSION");
    assert!(
        matrix.contains(&format!("MSRV {msrv}")),
        "update docs/compatibility.md to match Cargo.toml rust-version {msrv}"
    );

    for network in sororewind::network::Network::ALL {
        assert!(
            matrix.contains(network.default_rpc_url()),
            "add the {} default RPC to docs/compatibility.md",
            network.name()
        );
    }
}
