use soroban_sdk::{Env, Symbol, symbol_short, IntoVal};
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use crate::{TimeLockedUpgradeContract, TimeLockedUpgradeContractClient, UPGRADE_DELAY_SECONDS, DEFAULT_HEARTBEAT_INTERVAL};

/// Helper: advance the ledger timestamp by `delta` seconds.
fn advance_ledger_timestamp(env: &Env, delta: u64) {
    let current_ts = env.ledger().timestamp();
    env.ledger().set(LedgerInfo {
        timestamp: current_ts + delta,
        protocol_version: env.ledger().protocol_version(),
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: u32::MAX,
    });
}

// ═════════════════════════════════════════════════════════════════════════════
// Existing tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_initialize_and_basic_functionality() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);

    client.initialize(&admin);

    let data = client.get_data();
    assert_eq!(data.admin, admin);
    assert_eq!(data.value, 0);

    client.set_value(&42, &admin);
    let data = client.get_data();
    assert_eq!(data.value, 42);
}

#[test]
fn test_propose_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);

    client.propose_upgrade(&new_wasm_hash, &admin);

    let pending = client.get_pending_upgrade();
    assert!(pending.is_some());

    let pending_upgrade = pending.unwrap();
    assert_eq!(pending_upgrade.new_wasm_hash, new_wasm_hash);
    assert_eq!(pending_upgrade.proposer, admin);

    let remaining = client.get_upgrade_timelock_remaining();
    assert!(remaining.is_some());
    assert_eq!(remaining.unwrap(), UPGRADE_DELAY_SECONDS);
}

#[test]
fn test_execute_upgrade_after_timelock() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);

    client.propose_upgrade(&new_wasm_hash, &admin);

    // Fast forward time by 48 hours
    advance_ledger_timestamp(&env, UPGRADE_DELAY_SECONDS);

    // Timelock should be satisfied
    let remaining = client.get_upgrade_timelock_remaining();
    assert_eq!(remaining.unwrap(), 0);
}

#[test]
fn test_cancel_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);

    client.propose_upgrade(&new_wasm_hash, &admin);
    assert!(client.get_pending_upgrade().is_some());

    client.cancel_upgrade(&admin);

    assert!(client.get_pending_upgrade().is_none());
    assert!(client.get_upgrade_timelock_remaining().is_none());
}

#[test]
fn test_timelock_countdown() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);

    client.propose_upgrade(&new_wasm_hash, &admin);

    let remaining = client.get_upgrade_timelock_remaining().unwrap();
    assert_eq!(remaining, UPGRADE_DELAY_SECONDS);

    advance_ledger_timestamp(&env, 24 * 60 * 60);

    let remaining = client.get_upgrade_timelock_remaining().unwrap();
    assert_eq!(remaining, 24 * 60 * 60);

    advance_ledger_timestamp(&env, 24 * 60 * 60);

    let remaining = client.get_upgrade_timelock_remaining().unwrap();
    assert_eq!(remaining, 0);
}

// ═════════════════════════════════════════════════════════════════════════════
// Heartbeat Verification tests (Issue #188)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_heartbeat_fresh_data() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let asset = symbol_short!("NGN");

    // Update heartbeat
    client.update_heartbeat(&asset, &admin);

    // Data should be fresh immediately after update
    assert!(client.is_data_fresh(&asset));

    // Verify timestamp was recorded
    let ts = client.get_last_update_timestamp(&asset);
    assert!(ts.is_some());
    assert_eq!(ts.unwrap(), env.ledger().timestamp());
}

#[test]
fn test_heartbeat_stale_data() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let asset = symbol_short!("KES");

    // Update heartbeat at current time
    client.update_heartbeat(&asset, &admin);
    assert!(client.is_data_fresh(&asset));

    // Fast-forward past the default heartbeat interval (5 min = 300s) + 1
    advance_ledger_timestamp(&env, DEFAULT_HEARTBEAT_INTERVAL + 1);

    // Data should now be stale
    assert!(!client.is_data_fresh(&asset));
}

#[test]
fn test_heartbeat_never_updated() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let asset = symbol_short!("GHS");

    // No heartbeat recorded → should be stale
    assert!(!client.is_data_fresh(&asset));
    assert!(client.get_last_update_timestamp(&asset).is_none());
}

#[test]
fn test_heartbeat_custom_interval() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let asset = symbol_short!("CFA");

    // Verify default interval
    assert_eq!(client.get_heartbeat_interval(), DEFAULT_HEARTBEAT_INTERVAL);

    // Set a custom interval of 10 minutes (600 seconds)
    let custom_interval: u64 = 600;
    client.set_heartbeat_interval(&custom_interval, &admin);
    assert_eq!(client.get_heartbeat_interval(), custom_interval);

    // Update heartbeat
    client.update_heartbeat(&asset, &admin);
    assert!(client.is_data_fresh(&asset));

    // Fast-forward 301 seconds — stale with default, but fresh with custom
    advance_ledger_timestamp(&env, 301);
    assert!(client.is_data_fresh(&asset)); // Still fresh (301 < 600)

    // Fast-forward past the custom interval
    advance_ledger_timestamp(&env, 300); // total: 601
    assert!(!client.is_data_fresh(&asset)); // Now stale (601 > 600)
}

/*
#[test]
fn test_heartbeat_unauthorized_update() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    let unauthorized = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let asset = symbol_short!("NGN");

    // Non-admin tries to update heartbeat — should panic
    let args = soroban_sdk::vec![&env, asset.into_val(&env), unauthorized.into_val(&env)];
    let result = env.try_invoke_contract::<(), soroban_sdk::Error>(
        &contract_id,
        &soroban_sdk::Symbol::new(&env, "update_heartbeat"),
        args,
    );
    assert!(result.is_err());
}
*/

/*
#[test]
fn test_heartbeat_unauthorized_set_interval() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    let unauthorized = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    // Non-admin tries to set heartbeat interval — should panic
    let args = soroban_sdk::vec![&env, 600u64.into_val(&env), unauthorized.into_val(&env)];
    let result = env.try_invoke_contract::<(), soroban_sdk::Error>(
        &contract_id,
        &soroban_sdk::Symbol::new(&env, "set_heartbeat_interval"),
        args,
    );
    assert!(result.is_err());
}
*/

/*
#[test]
fn test_unauthorized_propose_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);
    
    let admin = soroban_sdk::Address::generate(&env);
    let unauthorized_user = soroban_sdk::Address::generate(&env);
    
    client.initialize(&admin);
    
    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[1u8; 32]);
    
    // Try to propose upgrade as unauthorized user - should fail
    let args = soroban_sdk::vec![&env, new_wasm_hash.into_val(&env), unauthorized_user.into_val(&env)];
    let result = env.try_invoke_contract::<(), soroban_sdk::Error>(
        &contract_id,
        &soroban_sdk::Symbol::new(&env, "propose_upgrade"),
        args,
    );
    assert!(result.is_err());
}
*/

/*
#[test]
fn test_unauthorized_set_value() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);
    
    let admin = soroban_sdk::Address::generate(&env);
    let unauthorized_user = soroban_sdk::Address::generate(&env);
    
    client.initialize(&admin);
    
    // Try to set value as unauthorized user - should fail
    let args = soroban_sdk::vec![&env, 42u64.into_val(&env), unauthorized_user.into_val(&env)];
    let result = env.try_invoke_contract::<(), soroban_sdk::Error>(
        &contract_id,
        &soroban_sdk::Symbol::new(&env, "set_value"),
        args,
    );
    assert!(result.is_err());
}
*/

#[test]
fn test_set_value_updates_heartbeat() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(&env, &contract_id);

    let admin = soroban_sdk::Address::generate(&env);
    client.initialize(&admin);

    let value_asset = symbol_short!("VALUE");

    // Before set_value, no heartbeat exists for "VALUE"
    assert!(!client.is_data_fresh(&value_asset));

    // Call set_value — should auto-record heartbeat
    client.set_value(&42, &admin);

    // Now the "VALUE" asset should have a fresh heartbeat
    assert!(client.is_data_fresh(&value_asset));
    assert!(client.get_last_update_timestamp(&value_asset).is_some());

    // Fast-forward past interval → data goes stale
    advance_ledger_timestamp(&env, DEFAULT_HEARTBEAT_INTERVAL + 1);
    assert!(!client.is_data_fresh(&value_asset));

    // Another set_value call refreshes the heartbeat
    client.set_value(&100, &admin);
    assert!(client.is_data_fresh(&value_asset));
}

// ═════════════════════════════════════════════════════════════════════════════
// Emergency Key Revocation tests
// ═════════════════════════════════════════════════════════════════════════════

fn setup_revocation(
    env: &Env,
) -> (
    TimeLockedUpgradeContractClient,
    soroban_sdk::Address, // admin (compromised)
    soroban_sdk::Address, // signer_a
    soroban_sdk::Address, // signer_b
    soroban_sdk::Address, // signer_c
    soroban_sdk::Address, // replacement
) {
    let contract_id = env.register_contract(None, TimeLockedUpgradeContract);
    let client = TimeLockedUpgradeContractClient::new(env, &contract_id);

    let admin = soroban_sdk::Address::generate(env);
    let signer_a = soroban_sdk::Address::generate(env);
    let signer_b = soroban_sdk::Address::generate(env);
    let signer_c = soroban_sdk::Address::generate(env);
    let replacement = soroban_sdk::Address::generate(env);

    client.initialize(&admin);
    client.register_signer(&signer_a, &admin);
    client.register_signer(&signer_b, &admin);
    client.register_signer(&signer_c, &admin);

    (client, admin, signer_a, signer_b, signer_c, replacement)
}

#[test]
fn test_register_and_get_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, signer_a, signer_b, signer_c, _replacement) = setup_revocation(&env);

    let signers = client.get_signers();
    assert_eq!(signers.len(), 3);
    assert!(signers.iter().any(|s| s == signer_a));
    assert!(signers.iter().any(|s| s == signer_b));
    assert!(signers.iter().any(|s| s == signer_c));
}

#[test]
fn test_remove_signer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, _replacement) = setup_revocation(&env);

    client.remove_signer(&signer_a, &admin);
    let signers = client.get_signers();
    assert_eq!(signers.len(), 2);
    assert!(!signers.iter().any(|s| s == signer_a));
}

#[test]
fn test_propose_revocation_creates_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);

    let proposal = client.get_revocation_proposal().unwrap();
    assert_eq!(proposal.target, admin);
    assert_eq!(proposal.replacement, replacement);
    assert_eq!(proposal.proposer, signer_a);
    // Proposer auto-votes
    assert_eq!(proposal.votes.len(), 1);
    assert_eq!(proposal.votes.get(0).unwrap(), signer_a);
}

#[test]
fn test_full_revocation_lifecycle_majority_vote() {
    let env = Env::default();
    env.mock_all_auths();
    // 3 signers → threshold = 3/2 + 1 = 2
    let (client, admin, signer_a, signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a); // vote 1
    // Proposal still active (1 < 2)
    assert!(client.get_revocation_proposal().is_some());

    client.vote_revocation(&signer_b); // vote 2 → threshold reached, auto-executes

    // Proposal cleared
    assert!(client.get_revocation_proposal().is_none());
    // Admin replaced
    let data = client.get_data();
    assert_eq!(data.admin, replacement);
}

#[test]
fn test_revocation_requires_majority_before_execution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    // Only 1 vote so far — execute_revocation should panic
    let result = client.try_execute_revocation(&signer_a);
    assert!(result.is_err());
    // Admin unchanged
    assert_eq!(client.get_data().admin, admin);
}

#[test]
fn test_duplicate_vote_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    // signer_a already voted as proposer
    let result = client.try_vote_revocation(&signer_a);
    assert!(result.is_err());
}

#[test]
fn test_non_signer_cannot_propose() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);
    let outsider = soroban_sdk::Address::generate(&env);

    let result = client.try_propose_revocation(&admin, &replacement, &outsider);
    assert!(result.is_err());
}

#[test]
fn test_non_signer_cannot_vote() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);
    let outsider = soroban_sdk::Address::generate(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    let result = client.try_vote_revocation(&outsider);
    assert!(result.is_err());
}

#[test]
fn test_cannot_propose_when_proposal_active() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    let result = client.try_propose_revocation(&admin, &replacement, &signer_b);
    assert!(result.is_err());
}

#[test]
fn test_proposer_can_cancel_revocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    assert!(client.get_revocation_proposal().is_some());

    client.cancel_revocation(&signer_a);
    assert!(client.get_revocation_proposal().is_none());
    // Admin unchanged
    assert_eq!(client.get_data().admin, admin);
}

#[test]
fn test_outsider_cannot_cancel_revocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);
    let outsider = soroban_sdk::Address::generate(&env);

    client.propose_revocation(&admin, &replacement, &signer_a);
    let result = client.try_cancel_revocation(&outsider);
    assert!(result.is_err());
}

#[test]
fn test_execute_revocation_after_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    // 3 signers → threshold = 2
    let (client, admin, signer_a, signer_b, _signer_c, replacement) = setup_revocation(&env);

    client.propose_revocation(&admin, &replacement, &signer_a); // vote 1
    client.vote_revocation(&signer_b); // vote 2 → auto-executes

    // Confirm admin is now replacement
    assert_eq!(client.get_data().admin, replacement);
    assert!(client.get_revocation_proposal().is_none());
}

#[test]
fn test_target_must_be_current_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, signer_a, _signer_b, _signer_c, replacement) = setup_revocation(&env);
    let random = soroban_sdk::Address::generate(&env);

    // `random` is not the admin
    let result = client.try_propose_revocation(&random, &replacement, &signer_a);
    assert!(result.is_err());
}
