---
name: Report a divergence
about: Report a replay that disagrees with the network
labels: bug
---

**Transaction**

- Hash:
- Network (`testnet`, `mainnet`, `futurenet`, or `local`):
- RPC provider (omit API keys):
- sororewind version (`sororewind --version`):

**What differed?**

<!-- Include the reported divergence and what you expected. -->

**JSON report**

<!-- Run `sororewind <hash> --network <network> --format json` and paste the output. -->

```json
paste sanitized JSON output here
```

**Verbose log**

<!-- Run `sororewind <hash> --network <network> -vv 2>&1` and paste the relevant output. -->

```text
paste sanitized -vv output here
```

Remove API keys, credentials, and other sensitive information before posting.
