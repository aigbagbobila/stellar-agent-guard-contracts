# Spend Limits

Stellar Agent Guard provides two tiers of spend enforcement for SAC token transfers, both enforced inside `__check_auth` before any value moves.

## Per-transaction cap

The `per_tx_cap` field sets a maximum amount for any single SAC token transfer call. If the transfer amount exceeds this cap, the transaction is blocked with `PerTxCapExceeded`.

- `per_tx_cap: 0` disables the cap (no per-transaction limit).
- The cap applies to each individual `transfer` or `transfer_from` call, not to the batch of contexts in a single authorization.
- A blocked transaction does **not** consume the rolling window — the staged-admission pattern ensures window accounting only happens for admitted transfers.

```
Policy: per_tx_cap = 1000
Transfer 500 → Allowed (500 ≤ 1000)
Transfer 1100 → Blocked (1100 > 1000)
```

## Rolling window cap

The `window_cap` field sets a maximum total spend within a rolling time window of `window_secs` seconds. This is a **genuinely rolling** window, not a fixed-bucket reset.

### Why rolling matters

A fixed 86,400-second bucket (reset-at-midnight style) is a **different guarantee** from a rolling window. Under a fixed bucket, spend at 23:59 and spend at 00:01 are never counted together even though they are two minutes apart. Under a rolling window, any two spends within any 86,400-second span are counted together. The two guarantees diverge in exactly the burst-boundary cases a spend guard exists to catch.

### How it works

- Entries are append-ordered by unix ledger time (`env.ledger().timestamp()`, 1s granularity).
- On every evaluation: while `entries[0].ts + window_secs <= now`, pop from the front and subtract from `total`. Evaluation is lazy — no cron, no background writes; the O(expired) pruning cost amortizes over accesses.
- A new spend coalesces into the trailing entry when it shares the same second (`entries.last().ts == now`), so dense bursts in one second stay one entry.
- **Boundedness backstop:** `MAX_WINDOW_ENTRIES = 8192`. If a write would exceed it, the two oldest entries are merged into one whose `ts` is the **newer** of the two and whose `amount` is the sum. Merging forward over-counts (the older amount then expires later than it truly should), which can only make enforcement stricter than policy — it can never admit spend that policy would reject.

### Example

```
Policy: window_cap = 150, window_secs = 60

t=0:    transfer 50  → Allowed (total: 50)
t=30:   transfer 110 → Blocked (50 + 110 = 160 > 150)
t=70:   transfer 50  → Allowed (first entry at t=0 expired: 70 - 60 = 10 > 0)
```

The window is genuinely rolling — the transfer at `t=70` succeeds because the spend at `t=0` has expired from the window.

### Invariant

For every authorization decision, `total` after any admission equals the sum of `entries[i].amount` over entries with `ts + window_secs > now`, and a new asset transfer is admitted only if that running total plus the transfer amount ≤ `window_cap`.

## Configuration interaction

| `per_tx_cap` | `window_cap` | Behavior |
|---|---|---|
| 0 | 0 | No spend limits at all |
| > 0 | 0 | Per-tx cap only |
| 0 | > 0 | Window cap only |
| > 0 | > 0 | Both enforced (per-tx checked first, then window) |

If `window_cap > 0`, then `window_secs` must also be > 0 (validated by `set_policy`).

## Scope

Both caps apply **only to SAC token transfers** (`transfer` / `transfer_from`). For other Soroban contract calls, the policy engine still enforces window and pause state, but per-call amount limits are not applied. See [Enforcement Scope](../enforcement-scope.md) for details.
