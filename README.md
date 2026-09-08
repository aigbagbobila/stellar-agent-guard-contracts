<p align="center">
<a href="https://github.com/aigbagbobila/stellar-agent-guard-contracts/actions/workflows/ci.yml">
<img src="https://github.com/aigbagbobila/stellar-agent-guard-contracts/actions/workflows/ci.yml/badge.svg" alt="CI"/>
</a>
<a href="LICENSE-MIT">
<img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0"/>
</a>
<a href="https://www.rust-lang.org/">
<img src="https://img.shields.io/badge/rust-1.85%2B-blue" alt="Rust 1.85+"/>
</a>
</p>

# stellar-agent-guard-contracts

**Non-custodial spend guardrails for autonomous agents on Stellar, enforced inside the
agent's own smart account.**

An autonomous agent holds funds in a Soroban **custom account** contract. Every
transaction the agent signs is routed by the host through the account's
`__check_auth` before it can touch any protocol — and that function enforces the
policy: per-transaction spend caps, a genuinely rolling window cap, recipient and
asset allowlists, a protocol/function allowlist, a pause switch, and a dead-man
switch. This is the circuit breaker for runaway agent spending: a compromised or
misbehaving agent key can move funds only within the limits the policy admin set,
and only until the freeze or heartbeat-expiry condition fires.

## 🎯 What makes this different

The enforcement point is the **account itself**, not a wrapper or a wallet
front-end. The agent's Ed25519 public key is registered inside a smart-wallet
contract implementing Soroban's `CustomAccountInterface`; from then on the
agent's address **is** the contract's address, and the Soroban host runs
`__check_auth` for every `require_auth` on that address — the guard is
on-chain and unbypassable, executing before any value moves.

Phase 0 research identified the related projects in this space — SpendGuard,
`stellarspend-contracts`, `soroban-vault`, `watchdog`, and `agentguard`. None of
them enforce spend policy inside the account's own `__check_auth`:
- **Watchdog** is the closest in intent (guarding autonomous agents), and its
  own roadmap explicitly lists `__check_auth`-based custom-account enforcement
  as its **unbuilt future direction** — this project implements that mechanism
  today (Phase 0 research finding).
- The others are vault/policy **wrapper** designs: funds sit in a vault or
  proxy contract, which adds a custody hop and a new surface to audit, and
  their per-call checks run in front of the target protocol rather than in the
  account the agent actually controls.

This project is **non-custodial**: funds live in the agent's own smart account
(no third-party vault, no `top_up`, no deposit step), there are no per-protocol
proxy wrapper contracts, and the policy admin holds **no fund-moving authority**
— admin functions change policy and freeze state only. The full architecture is
specified in [`SPEC.md`](SPEC.md).

> ⚠️ **Disclaimer:** This contract is **unaudited** and handles fund access.
> Do not deploy to mainnet with real funds until it has been audited. See
> [SECURITY.md](SECURITY.md).

## Enforcement scope — read this before relying on the caps

Full recipient/amount enforcement — spend caps, allowlists, per-transaction limits — is
native and automatic for SAC token transfers (`transfer`/`transfer_from`), since these are
the calls whose arguments the Soroban auth context exposes for inspection. For other Soroban
contract calls made by the guarded account (arbitrary DEX/lending/protocol calls), the
policy engine enforces the protocol allowlist — the account is default-deny, so every call
must match an allowlisted contract and, where configured, an allowlisted function — plus the
active-window, pause, freeze, and dead-man gates. Per-call amount/recipient limits and
rolling-window spend accounting are not applied to those calls, because the amount is not
available in the auth context in any trustworthy way. Extending fine-grained enforcement to
arbitrary calls is tracked as a v2 item, not implied as already covered.

This boundary is an inherent property of the platform (the auth context does not expose
arbitrary call arguments generically), not a gap this project hides or overclaims — the same
scope statement appears in SPEC §2 in the same terms.

## Quick Start

```bash
# Clone
git clone https://github.com/aigbagbobila/stellar-agent-guard-contracts.git
cd stellar-agent-guard-contracts

# Test, lint, format (phase 1 exit gates)
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check

# Build the contract wasm (Soroban 27 targets wasm32v1-none)
cargo build --release --target wasm32v1-none

# Build the agent-tx submission helper (signs Soroban auth entries for the
# custom-account address — the stellar CLI cannot)
cargo build --release --manifest-path tools/agent-tx/Cargo.toml
```

### Submitting a guarded transfer

The `stellar` CLI cannot sign Soroban auth entries whose address is a
**contract** (the guard), so guarded submissions go through `tools/agent-tx`,
which simulates (running the real `__check_auth` against live state), signs the
guard's `SorobanAuthorizationEntry` with the registered agent key, and submits:

```bash
agent-tx transfer \
    --guard CAYJZT4XH5SWDXNR7MZJCCUBIDAT2KZDDUTZ7OZQEMKCPJGD4P3X4CU7 \
    --token CBLQLJAG72M4XQRJMQHSKYIFVHQD7LNTNOQH2GRMCMBWMSLBSLTGTJC7 \
    --to GDUYLFVFLVISVOM5FK5KTBA446VQQ7NBRRFMLNLKLISKL26LJGKUVRRX \
    --amount 50
```

Policy violations surface as **blocked, pre-broadcast** (`--expect-blocked` for
negative tests). This tool is the prototype of the Phase 2 SDK's signing path.

## ✅ Verified against live testnet

All five enforcement scenarios were proven against **real deployed contracts**
on Stellar testnet (protocol 28), with real transaction hashes and the guard's
own `event_auth_checked` events:

- **Guard contract**: `CAYJZT4XH5SWDXNR7MZJCCUBIDAT2KZDDUTZ7OZQEMKCPJGD4P3X4CU7`
- **Allowed transfer** — PASSED on-chain, e.g. tx
  `4c5759298c0364b01d386a5935b964532b04978ea595d96d904d9011f58d64b8`
  (ledger 4566285, `successful: true`)
- **Per-tx cap violation** — 1100 > cap 1000 → blocked pre-broadcast,
  `per_tx_cap_exceeded`
- **Rolling-window cap violation** — 50 + 110 = 160 > cap 150 in the 60s window
  → blocked, `window_cap_exceeded`
- **Allowlist violation** — transfer to a non-allowlisted recipient → blocked,
  `recipient_not_allowed`
- **Dead-man switch trigger + reversal** — heartbeat stopped past the 60s grace
  → blocked `heartbeat_expired`; admin `unfreeze`
  (tx `dd327d32b18bfc6cebdf6c956503fe5318e28f8a8bc86a88cb7ee42c5d46b5e5`)
  restored spending, then transfers allowed again
  (tx `b39457afa59f20d6ac90cd137e917c7efd51e27af4913c6c6308a6e5d0eff512`)

The complete record — contract IDs, all setup and scenario transaction hashes,
event output, and how to re-run — lives in
[`tests/fixtures/README.md`](tests/fixtures/README.md).

## Repository layout

- `src/` — contract: `__check_auth` (CustomAccount), policy engine, rolling
  window, dead-man switch, admin controls, events.
- `src/integration_tests.rs` — contract-level tests exercising the real
  `__check_auth` path, including Ed25519 auth-signature verification.
- `tools/agent-tx/` — submission helper that signs Soroban auth entries for the
  guard address and submits real testnet transactions.
- `tests/fixtures/README.md` — real testnet evidence for the five scenarios.
- `SPEC.md` — the full architecture specification.

## Testing & CI

30 unit + integration tests cover the decision table, the rolling-window
invariant (including the low-timestamp prune regression), auth-context parsing,
Ed25519 signature verification, freeze/reversal, and default deny.

```bash
cargo test
cargo clippy --all-targets --all-features # clippy all + pedantic deny
cargo fmt --check
```

Every push runs these gates on GitHub Actions (`.github/workflows/ci.yml`, job
`ci`): format → clippy → tests → contract build → tool build. `main` is
protected — the `ci` check must be green for changes to merge.

## Project Status

| Feature | Status |
|---------|--------|
| Custom-account `__check_auth` enforcement | ✅ |
| Per-tx spend cap | ✅ |
| Genuinely rolling window cap | ✅ |
| Recipient / asset allowlists | ✅ |
| Protocol / function allowlist (default-deny) | ✅ |
| Pause switch | ✅ |
| Dead-man switch + admin reversal | ✅ |
| Agent key rotation | ✅ |
| Verified against live testnet (5 scenarios) | ✅ |
| Fine-grained caps on non-SAC calls | 🚧 v2 |

## Topics

`stellar`, `soroban`, `smart-contracts`, `agent-security`, `security`

## Contact

- GitHub issues: <https://github.com/aigbagbobila/stellar-agent-guard-contracts/issues>
- Maintainer (GitHub): [@aigbagbobila](https://github.com/aigbagbobila)
- Security disclosures: see [SECURITY.md](SECURITY.md)

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at
your option.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for details on coding standards, commit
discipline, PR process, and project structure.

Looking for something to work on? The
[issue backlog](https://github.com/aigbagbobila/stellar-agent-guard-contracts/issues)
holds scoped issues with Summary / Acceptance Criteria / Tech Stack — good
first tasks for the Drips Stellar Wave contributor sprints.