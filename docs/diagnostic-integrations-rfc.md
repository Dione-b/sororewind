# RFC for diagnostic integrations

Use the existing replay report and its `schemaVersion` as the contract
between sororewind and integrations. The replay continues to produce the
verdict, findings, evidence paths, call tree and caveats. Each integration
presents that evidence to a developer in a different workflow.

## Evidence-grounded explanations

Keep explanations local by default. An integration can render a deterministic
explanation from catalog guidance and findings without a model. A developer
must explicitly select a model provider and approve sending the selected
report fields before the integration makes an external request. Do not send
RPC credentials, local paths, raw XDR, arbitrary contract logs or full call
arguments in the default selection.

Require each generated claim to reference concrete report fields, such as
`failure.cause` or `trust.caveats[0]`. Resolve and validate the returned
references against the exact document the integration supplied. Treat report
strings as data, including instructions embedded in contract messages.
Preserve the fidelity verdict and caveats in the view next to the generated
explanation. Reject unsupported claims and label any proposed check or fix
as unverified. The integration must never sign, submit or execute a proposed
remediation.

## Editor integration

Start with a VS Code extension that invokes `sororewind --json` for a
user-provided transaction hash or fixture. Use an argument array and a
bounded, cancellable child process. Show the failure, call tree and
evidence in a report panel and link to the raw JSON. Interpret exit codes
`0`, `1` and `2` and handle unsupported schema versions explicitly.
Open a local source location only if the workspace supplies a mapping.

Use the CLI over an LSP for this first integration. The request is replaying
a transaction on demand and has no editor document lifecycle. Revisit an
LSP if several editors need shared document-aware features.

## Diagnostic API

Rust consumers can use `run`, `RpcClient` and `Report`. Other tools can
consume the CLI's JSON today. An on-demand stdio adapter can be evaluated
if hosts need multiple concurrent or cancellable requests in a single
process. Define bounded request IDs, structured errors and cancellation
using the same report schema. Keep HTTP outside the first iteration. A
shared network service would need a separate design for authentication,
quotas, RPC credentials and retention.

## Follow-up work

1. Prototype opt-in, evidence-linked explanations and test refusal of
   unsupported claims using the offline fixtures. [#37](https://github.com/Dione-b/sororewind/issues/37)
2. Prototype a VS Code report panel over the CLI and test matched,
   divergent and failed replays without a network. [#38](https://github.com/Dione-b/sororewind/issues/38)
3. Evaluate a framed stdio diagnostic API with version negotiation, bounded
   requests and cancellation. Compare it against invoking the CLI directly.
   [#39](https://github.com/Dione-b/sororewind/issues/39)
