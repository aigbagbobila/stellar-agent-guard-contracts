# stellar-agent-guard-contracts

Soroban smart contracts for **Stellar Agent Guard** — non-custodial, account-level spend
guardrails for autonomous agents, built on Soroban native Custom Account Abstraction.

**Status: Phase 1 complete.** The policy engine is implemented and proven on Stellar testnet:
all five enforcement scenarios (allowed transfer, per-tx cap, rolling window cap, allowlist,
dead-man switch trigger + reversal) were exercised against live testnet contracts with real
transaction hashes and contract events. Evidence is recorded in
[`tests/fixtures/README.md`](tests/fixtures/README.md).

## Mechanism (settled)

The agent's keypair is registered inside a smart-wallet contract implementing the Soroban
`CustomAccount` trait; `__check_auth` is the enforcement vector every transaction routes
through before touching a target protocol. Funds remain in the agent's own smart account —
this is non-custodial, not a third-party vault, and there are no per-protocol proxy wrapper
contracts.

The guard enforces: per-transaction spend caps, a genuinely rolling 24h-style window cap
(window is configurable in seconds), recipient/asset allowlists, a protocol/function
allowlist, a pause switch, and a dead-man switch (heartbeat with admin-attested reversal).
Every call is evaluated by `__check_auth` and rejected pre-broadcast if it violates any
active rule. Full design in [`SPEC.md`](SPEC.md).

## Enforcement scope — read this before relying on the caps

Full recipient/amount enforcement — spend caps, allowlists, per-transaction limits — is
native and automatic for SAC token transfers (`transfer`/`transfer_from`), since these are
the calls whose arguments the Soroban auth context exposes for inspection. For other Soroban
contract calls made by the guarded account (arbitrary DEX/lending/protocol calls), the
policy engine still enforces window and pause state, but per-call amount/recipient limits are
not yet enforced — extending fine-grained enforcement to arbitrary calls is tracked as a v2
item, not implied as already covered.

This boundary is an inherent property of the platform (the auth context does not expose
arbitrary call arguments generically), not a gap this project hides or overclaims — the same
scope statement appears in SPEC §2 in the same terms.

## Repository layout

- `src/` — contract: `__check_auth` (CustomAccount), policy engine, rolling window,
  dead-man switch, admin controls, events.
- `src/integration_tests.rs` — contract-level tests that exercise the real
  `__check_auth` path, including Ed25519 auth-signature verification (SPEC §11).
- `tools/agent-tx/` — submission helper that signs Soroban auth entries for the guard
  (custom-account) address and submits real testnet transactions. The `stellar` CLI
  cannot sign auth entries whose address is a contract, so this tool fills that gap; it
  is the prototype of the Phase 2 SDK's signing path.
- `tests/fixtures/README.md` — the real testnet evidence for the five scenarios.
- `SPEC.md` — the full architecture specification.

## Building and testing

```sh
# local unit + integration tests (isolated, no network)
cargo test

# lint and format gates (phase 1 exit criteria)
cargo clippy --all-targets --all-features
cargo fmt --check

# contract wasm (Soroban 27 targets wasm32v1-none, not wasm32-unknown-unknown)
cargo build --release --target wasm32v1-none

# submission helper
cargo build --release --manifest-path tools/agent-tx/Cargo.toml
```

CI (`.github/workflows/ci.yml`, job `ci`) runs all of the above on push to `main` and on
pull requests.

## Testnet proof

See [`tests/fixtures/README.md`](tests/fixtures/README.md) for the complete evidence:
real contract IDs, real transaction hashes, and the guard's own `event_auth_checked`
events for all five scenarios.

## Related repos

- [stellar-agent-guard-sdk](../stellar-agent-guard-sdk) — integration bridge (signing,
  pre-flight interception, event telemetry). Phase 2.
- [stellar-agent-guard-dashboard](../stellar-agent-guard-dashboard) — operator interface.
  Phase 3.