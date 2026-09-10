# Custom Account Enforcement

## How it works

Stellar Agent Guard uses **Soroban native Custom Account Abstraction** to enforce spend policy. The agent's Ed25519 public key is registered inside a smart-wallet contract implementing the `CustomAccount` trait. From then on, the agent's **address is the contract's address**: any transaction that needs the agent to authorize an action (any `require_auth` on that address) is routed by the host through the contract's `__check_auth` before the action can touch a target protocol.

`__check_auth` is the **single enforcement vector** — there is no other path for the account to authorize anything.

## The CustomAccount interface

The contract implements Soroban's `CustomAccountInterface`:

```rust
impl CustomAccountInterface for PolicyEngine {
    type Signature = BytesN<64>;
    type Error = Error;

    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signatures: Self::Signature,
        auth_contexts: Vec<Context>,
    ) -> Result<(), Error>;
}
```

The host invokes `__check_auth` once per authorization the account must approve, supplying the contexts of the calls being authorized. The contract:

1. **Verifies the agent's Ed25519 signature** over `signature_payload` via `env.crypto().ed25519_verify` — a wrong key or bad signature traps the frame before any policy evaluation.
2. **Evaluates the policy decision table** over every auth context (see [Architecture](../architecture.md)).
3. Returns `Ok(())` to approve, or `Err(reason)` to reject the entire transaction.

## Non-custodial guarantee

- **Funds live in the agent's own smart account** (balances held at the contract address by Stellar Asset Contracts).
- **No third-party vault**, no `top_up`, no deposit step.
- The policy admin holds **no fund-moving authority of any kind**: admin functions change policy and freeze state only.
- Only the registered agent key can move funds, and only within the policy enforced in `__check_auth`.

## No per-protocol proxy wrappers

The account calls target protocols directly; the policy contract is the account itself, not a wrapper in front of anything. There is no proxy pattern.

## No classic-op gap

A contract account cannot perform classic Stellar operations (its address has no Ed25519 key of its own), so every action of the account is a Soroban invocation and therefore passes through `__check_auth`. There is no classic-op enforcement gap to configure.

## What this means in practice

When an agent tries to transfer tokens:

1. The agent signs a transaction that calls `transfer` on a Stellar Asset Contract.
2. The SAC's `transfer` function calls `from.require_auth()`.
3. The host sees that `from` is a contract address (the guard), so it invokes the guard's `__check_auth` with the auth context of the transfer call.
4. The guard verifies the agent's signature, then checks the policy: is the asset allowlisted? Is the recipient allowed? Does the transfer exceed the per-tx cap or window cap?
5. If everything passes, `__check_auth` returns `Ok(())`, and the transfer executes. If anything fails, `__check_auth` returns `Err(reason)`, and the transaction is rejected pre-broadcast.

This all happens inside the Soroban host — there is no off-chain component, no separate service, no bypass path.
