# Allowlists

Stellar Agent Guard has two distinct tiers of allowlisting, reflecting the different enforcement capabilities available for different call types.

## Tier 1: Recipient allowlist (SAC transfers only)

The `recipients` field in `PolicyConfig` lists the addresses that the guarded account is allowed to send SAC tokens to. When `allow_any_recipient: false`, any `transfer` or `transfer_from` to an address not in this list is blocked with `RecipientNotAllowed`.

```
Policy: recipients = [alice, bob], allow_any_recipient = false

transfer(to: alice, amount: 50) → Allowed
transfer(to: charlie, amount: 50) → Blocked (RecipientNotAllowed)
```

### The escape hatch

Setting `allow_any_recipient: true` skips the recipient check. Spend caps still apply — this only disables the recipient filter, not the amount enforcement.

```
Policy: recipients = [], allow_any_recipient = true, per_tx_cap = 100

transfer(to: anyone, amount: 50) → Allowed (caps still checked)
transfer(to: anyone, amount: 150) → Blocked (PerTxCapExceeded)
```

### Why only SAC transfers?

The recipient allowlist only works for SAC token transfers because these are the calls whose arguments the Soroban auth context exposes for inspection. For arbitrary protocol calls, the arguments are not interpretable — there is no reliable way to determine "who is the recipient" from a generic contract call's args.

## Tier 2: Protocol/function allowlist (any call)

The `protocols` field lists the non-asset contracts the guarded account is allowed to call. This is a **default-deny** environment: any contract not in this list is blocked with `UnknownContract`.

Each protocol rule can optionally restrict which functions are allowed:

```rust
pub struct ProtocolRule {
    pub contract: Address,
    pub fns: Option<Vec<Symbol>>,  // None = any function; Some = function allowlist
}
```

### Examples

```
Policy: protocols = [
    ProtocolRule { contract: dex_contract, fns: Some(["swap"]) },
    ProtocolRule { contract: lending_contract, fns: None }  // any function
]

call(dex_contract, "swap", args)     → Allowed
call(dex_contract, "drain", args)    → Blocked (FunctionNotAllowed)
call(lending_contract, "borrow", ..)  → Allowed (any function)
call(unknown_contract, "foo", args)   → Blocked (UnknownContract)
```

### What the protocol allowlist does NOT enforce

For non-SAC calls, the protocol allowlist checks:
- ✅ That the contract is in the allowlist
- ✅ That the function is in the function allowlist (if configured)
- ✅ That the account is not paused, frozen, or past the active window

It does **not** check:
- ❌ Per-call amount limits (the amount is not available in the auth context for arbitrary calls)
- ❌ Recipient limits (the recipient is not interpretable from arbitrary call args)

This is an inherent property of the platform — see [Enforcement Scope](../enforcement-scope.md).

## Self-calls

The guard contract allows one self-call: `heartbeat`. Any other self-function (e.g., `set_policy` called through `__check_auth`) is blocked with `SelfFunctionNotAllowed`.

## Create-contract calls

Contract creation by the guarded account is denied in v1 (`CreateContractNotAllowed`). An account that may not call unknown contracts should not create them.

## The two-tier distinction

| | SAC transfers | Protocol calls |
|---|---|---|
| Recipient allowlist | ✅ | ❌ |
| Per-tx amount cap | ✅ | ❌ |
| Window spend accounting | ✅ | ❌ |
| Contract allowlist | ✅ (via `assets`) | ✅ (via `protocols`) |
| Function allowlist | ✅ (transfer/transfer_from only) | ✅ |
| Pause/freeze/active window | ✅ | ✅ |
| Default deny | ✅ | ✅ |
