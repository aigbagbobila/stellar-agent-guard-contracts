# stellar-agent-guard-contracts

Soroban smart contracts for **Stellar Agent Guard** — non-custodial, account-level spend
guardrails for autonomous agents, built on Soroban native Custom Account Abstraction.

**Status: Phase 0 scaffold only. No contract code yet.** Phase 1 (architecture spec, policy
engine implementation, testnet proof of the five enforcement scenarios) has not started.

## Mechanism (settled)

The agent's keypair is registered inside a smart-wallet contract implementing the Soroban
`CustomAccount` trait; `__check_auth` is the enforcement vector every transaction routes
through before touching a target protocol. Funds remain in the agent's own smart account —
this is non-custodial, not a third-party vault, and there are no per-protocol proxy wrapper
contracts.

See the [SDK](../stellar-agent-guard-sdk) and [Dashboard](../stellar-agent-guard-dashboard)
repos for the integration and operator layers.
