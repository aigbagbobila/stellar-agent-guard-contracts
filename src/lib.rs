#![no_std]

//! stellar-agent-guard-contracts — a Soroban **custom account** that enforces
//! agent spend policy inside `__check_auth`. See SPEC.md.

mod engine;
mod types;
mod window;

use engine::{contains_addr, decide, AccountState, Decision};
use soroban_sdk::auth::{Context, ContractContext, CustomAccountInterface};
use soroban_sdk::{
    contract, contractevent, contractimpl, panic_with_error, vec, Address, Env, IntoVal, Symbol,
    TryFromVal, Val,
};
use types::{CheckResult, DataKey, Error, PolicyConfig, Status, WindowState};
use window::Ledger;

// ── Contract events (SPEC §9) ────────────────────────────────────────────

#[contractevent]
#[derive(Clone)]
struct EventAuth {
    #[topic]
    name: Symbol,
    #[topic]
    result: Symbol,
    #[topic]
    reason: Symbol,
}

#[contractevent]
#[derive(Clone)]
struct EventAdmin {
    #[topic]
    name: Symbol,
    #[topic]
    by: Address,
}

#[contractevent]
#[derive(Clone)]
struct EventHeartbeat {
    #[topic]
    name: Symbol,
    at: u64,
}

// ── Storage helpers ──────────────────────────────────────────────────────
// Admin / AgentSigner / Initialized live in instance storage (auto-TTL on
// every invocation); the rest live in persistent storage with explicit TTL
// extension on every write (SPEC §3).

fn persist_set(env: &Env, key: &DataKey, val: &impl soroban_sdk::IntoVal<Env, Val>) {
    let seq = env.ledger().sequence();
    let target = seq.saturating_add(env.storage().max_ttl());
    env.storage().persistent().set(key, val);
    env.storage().persistent().extend_ttl(key, target, target);
}

fn persist_get<T: soroban_sdk::TryFromVal<Env, Val>>(env: &Env, key: &DataKey) -> Option<T> {
    env.storage().persistent().get(key)
}

// ── Policy config validation (SPEC §8) ───────────────────────────────────

fn has_dup<T: PartialEq + TryFromVal<Env, Val> + IntoVal<Env, Val>>(
    env: &Env,
    items: &soroban_sdk::Vec<T>,
) -> bool {
    let n = items.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if let (Some(a), Some(b)) = (items.get(i), items.get(j)) {
                if a == b {
                    return true;
                }
            }
        }
    }
    let _ = env;
    false
}

fn validate_config(env: &Env, cfg: &PolicyConfig) -> Result<(), Error> {
    if cfg.per_tx_cap < 0 || cfg.window_cap < 0 {
        return Err(Error::InvalidConfig);
    }
    if cfg.window_cap > 0 && cfg.window_secs == 0 {
        return Err(Error::InvalidConfig);
    }
    if cfg.active_until != 0 && cfg.active_until <= cfg.active_from {
        return Err(Error::InvalidConfig);
    }
    let self_addr = env.current_contract_address();
    if contains_addr(&cfg.assets, &self_addr) {
        return Err(Error::InvalidConfig);
    }
    for i in 0..cfg.protocols.len() {
        if let Some(rule) = cfg.protocols.get(i) {
            if rule.contract == self_addr {
                return Err(Error::InvalidConfig);
            }
        }
    }
    if has_dup(env, &cfg.assets) || has_dup(env, &cfg.recipients) {
        return Err(Error::InvalidConfig);
    }
    let mut contracts: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(env);
    for i in 0..cfg.protocols.len() {
        if let Some(rule) = cfg.protocols.get(i) {
            for j in 0..contracts.len() {
                if let Some(existing) = contracts.get(j) {
                    if existing == rule.contract {
                        return Err(Error::InvalidConfig);
                    }
                }
            }
            contracts.push_back(rule.contract.clone());
            if let Some(fns) = &rule.fns {
                if fns.is_empty() || has_dup(env, fns) {
                    return Err(Error::InvalidConfig);
                }
            }
        }
    }
    Ok(())
}

// ── Events (SPEC §9) ─────────────────────────────────────────────────────

fn emit_auth(env: &Env, allowed: bool, reason: Option<&Error>) {
    let res = if allowed { "allowed" } else { "blocked" };
    let reason = reason.map_or("", |e| e.reason());
    EventAuth {
        name: Symbol::new(env, "auth_checked"),
        result: Symbol::new(env, res),
        reason: Symbol::new(env, reason),
    }
    .publish(env);
}

fn emit_policy(env: &Env, name: &str, by: &Address) {
    EventAdmin {
        name: Symbol::new(env, name),
        by: by.clone(),
    }
    .publish(env);
}

fn emit_no_by(env: &Env, name: &str, data: u64) {
    EventHeartbeat {
        name: Symbol::new(env, name),
        at: data,
    }
    .publish(env);
}

// ── Window ledger persistence ────────────────────────────────────────────

fn load_ledger(env: &Env) -> Ledger {
    match persist_get::<WindowState>(env, &DataKey::Window) {
        Some(state) => Ledger::from_entries(env, state.entries),
        None => Ledger::empty(env),
    }
}

fn save_ledger(env: &Env, ledger: &Ledger) {
    persist_set(
        env,
        &DataKey::Window,
        &WindowState {
            total: ledger.total,
            entries: ledger.entries.clone(),
        },
    );
}

// ── Contract ─────────────────────────────────────────────────────────────

#[contract]
pub struct PolicyEngine;

#[contractimpl]
#[allow(clippy::needless_pass_by_value)] // contract ABI requires owned args
impl PolicyEngine {
    // ── Lifecycle ────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, agent: Address) {
        if persist_get::<bool>(&env, &DataKey::Initialized).unwrap_or(false) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::AgentSigner, &agent);
        persist_set(&env, &DataKey::Initialized, &true);
        persist_set(&env, &DataKey::AdminFrozen, &false);
        persist_set(&env, &DataKey::LastHeartbeat, &0u64);
        emit_policy(&env, "initialized", &admin);
    }

    // ── Policy management (admin only) ───────────────────────────────────

    fn admin_or_panic(env: &Env) -> Address {
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match admin {
            Some(a) => {
                a.require_auth();
                a
            }
            None => panic_with_error!(env, Error::NotInitialized),
        }
    }

    pub fn set_policy(env: Env, config: PolicyConfig) {
        let admin = Self::admin_or_panic(&env);
        validate_config(&env, &config).unwrap_or_else(|e| panic_with_error!(&env, e));
        persist_set(&env, &DataKey::Policy, &config);
        // Fresh window on every policy change; heartbeat clock starts now so a
        // newly enabled dead-man switch grants full grace.
        save_ledger(&env, &Ledger::empty(&env));
        let now = env.ledger().timestamp();
        persist_set(&env, &DataKey::LastHeartbeat, &now);
        emit_policy(&env, "policy_set", &admin);
    }

    pub fn revoke_policy(env: Env) {
        let admin = Self::admin_or_panic(&env);
        env.storage().persistent().remove(&DataKey::Policy);
        env.storage().persistent().remove(&DataKey::Window);
        emit_policy(&env, "policy_revoked", &admin);
    }

    pub fn rotate_agent_key(env: Env, new_agent: Address) {
        let admin = Self::admin_or_panic(&env);
        env.storage()
            .instance()
            .set(&DataKey::AgentSigner, &new_agent);
        emit_policy(&env, "agent_rotated", &admin);
    }

    // ── Dead-man switch / freeze (SPEC §5) ───────────────────────────────

    pub fn heartbeat(env: Env) {
        // Routes through __check_auth (a self-call): the host verifies the
        // registered agent signer and the engine applies account gates.
        env.current_contract_address().require_auth();
        let now = env.ledger().timestamp();
        persist_set(&env, &DataKey::LastHeartbeat, &now);
        emit_no_by(&env, "heartbeat", now);
    }

    pub fn freeze(env: Env) {
        let admin = Self::admin_or_panic(&env);
        persist_set(&env, &DataKey::AdminFrozen, &true);
        emit_policy(&env, "frozen", &admin);
    }

    pub fn unfreeze(env: Env) {
        let admin = Self::admin_or_panic(&env);
        persist_set(&env, &DataKey::AdminFrozen, &false);
        let now = env.ledger().timestamp();
        persist_set(&env, &DataKey::LastHeartbeat, &now);
        emit_policy(&env, "unfrozen", &admin);
    }

    // ── Read / advisory ──────────────────────────────────────────────────

    #[allow(clippy::must_use_candidate)] // public read surface
    pub fn policy(env: Env) -> Option<PolicyConfig> {
        persist_get(&env, &DataKey::Policy)
    }

    #[allow(clippy::must_use_candidate)] // public read surface
    pub fn status(env: Env) -> Status {
        let has_policy = persist_get::<PolicyConfig>(&env, &DataKey::Policy).is_some();
        let admin_frozen = persist_get::<bool>(&env, &DataKey::AdminFrozen).unwrap_or(false);
        let last_heartbeat = persist_get::<u64>(&env, &DataKey::LastHeartbeat).unwrap_or(0);
        let now = env.ledger().timestamp();
        let grace =
            persist_get::<PolicyConfig>(&env, &DataKey::Policy).map_or(0, |c| c.dms_grace_secs);
        Status {
            has_policy,
            admin_frozen,
            heartbeat_expired: grace > 0
                && last_heartbeat != 0
                && now.saturating_sub(last_heartbeat) > grace,
            last_heartbeat,
            now,
        }
    }

    /// Pure pre-flight of the asset-transfer decision path (no writes).
    #[allow(clippy::must_use_candidate)] // public read surface
    pub fn check(env: Env, asset: Address, to: Address, amount: i128) -> CheckResult {
        let Some(cfg) = persist_get::<PolicyConfig>(&env, &DataKey::Policy) else {
            return CheckResult::Blocked(Symbol::new(&env, Error::NoPolicy.reason()));
        };
        let frozen = persist_get::<bool>(&env, &DataKey::AdminFrozen).unwrap_or(false);
        let last_heartbeat = persist_get::<u64>(&env, &DataKey::LastHeartbeat).unwrap_or(0);
        let now = env.ledger().timestamp();
        let self_addr = env.current_contract_address();
        let mut ledger = load_ledger(&env);
        let call = transfer_context(&env, &asset, &to, amount);
        match decide(
            &env,
            &self_addr,
            Some(&cfg),
            &AccountState {
                admin_frozen: frozen,
                last_heartbeat,
            },
            &mut ledger,
            now,
            vec![&env, call],
        ) {
            Decision::Allowed => CheckResult::Allowed,
            Decision::Blocked(e) => CheckResult::Blocked(Symbol::new(&env, e.reason())),
        }
    }
}

/// Build the auth `Context` of a SAC `transfer` call for pre-flight checks.
fn transfer_context(env: &Env, asset: &Address, to: &Address, amount: i128) -> Context {
    let mut args: soroban_sdk::Vec<Val> = soroban_sdk::Vec::new(env);
    args.push_back(asset.clone().into_val(env)); // from
    args.push_back(to.clone().into_val(env));
    args.push_back(amount.into_val(env));
    Context::Contract(ContractContext {
        contract: asset.clone(),
        fn_name: Symbol::new(env, "transfer"),
        args,
    })
}

// ── Custom account enforcement (SPEC §7) ─────────────────────────────────

#[contractimpl]
#[allow(clippy::needless_pass_by_value)] // trait + contract ABI require owned args
impl CustomAccountInterface for PolicyEngine {
    type Signature = ();
    type Error = Error;

    fn __check_auth(
        env: Env,
        signature_payload: soroban_sdk::crypto::Hash<32>,
        signatures: Self::Signature,
        auth_contexts: soroban_sdk::Vec<Context>,
    ) -> Result<(), Error> {
        let _ = (signature_payload, signatures); // host-verified via delegate_auth
                                                 // 1. The transaction must delegate to exactly the registered agent
                                                 //    signer, and no one else.
        let delegates = env.custom_account().get_delegated_signers();
        if delegates.len() != 1 {
            emit_auth(&env, false, Some(&Error::Unauthorized));
            return Err(Error::Unauthorized);
        }
        let agent: Option<Address> = env.storage().instance().get(&DataKey::AgentSigner);
        let Some(agent) = agent else {
            emit_auth(&env, false, Some(&Error::NotInitialized));
            return Err(Error::NotInitialized);
        };
        if delegates.get(0) != Some(agent.clone()) {
            emit_auth(&env, false, Some(&Error::Unauthorized));
            return Err(Error::Unauthorized);
        }

        // 2. Policy snapshot + gate evaluation over every context.
        let Some(cfg) = persist_get::<PolicyConfig>(&env, &DataKey::Policy) else {
            emit_auth(&env, false, Some(&Error::NoPolicy));
            return Err(Error::NoPolicy);
        };
        let frozen = persist_get::<bool>(&env, &DataKey::AdminFrozen).unwrap_or(false);
        let last_heartbeat = persist_get::<u64>(&env, &DataKey::LastHeartbeat).unwrap_or(0);
        let now = env.ledger().timestamp();
        let self_addr = env.current_contract_address();

        let mut ledger = load_ledger(&env);
        match decide(
            &env,
            &self_addr,
            Some(&cfg),
            &AccountState {
                admin_frozen: frozen,
                last_heartbeat,
            },
            &mut ledger,
            now,
            auth_contexts,
        ) {
            Decision::Allowed => {
                // 3. Host-verify the agent signer (ed25519 signature, or
                //    contract-delegate auth).
                env.custom_account().delegate_auth(&agent);
                // 4. Persist window changes made by the decision.
                let had_window = persist_get::<WindowState>(&env, &DataKey::Window).is_some();
                let has_entries = ledger.len() > 0;
                if had_window || has_entries {
                    save_ledger(&env, &ledger);
                }
                emit_auth(&env, true, None);
                Ok(())
            }
            Decision::Blocked(e) => {
                emit_auth(&env, false, Some(&e));
                Err(e)
            }
        }
    }
}
