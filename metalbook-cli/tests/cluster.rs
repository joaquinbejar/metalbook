//! The `solana-rpc-client` cluster without a network (its mock sender), URL redaction on real
//! client errors, preflight logs, the committed devnet parameters file and the calendar checks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_macros,
    missing_docs
)]

use std::collections::HashMap;

use base64::Engine;
use metalbook_cli::params::ParamsFile;
use metalbook_cli::rpc::{client_error, redact, Cluster};
use metalbook_cli::{set_calendar_ix, submit, CliError, Rpc};
use metalbook_sdk::Address;
use serde_json::{json, Value};
use solana_keypair::Keypair;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::client_error::{Error as ClientError, ErrorKind};
use solana_rpc_client_api::request::{RpcError, RpcRequest, RpcResponseErrorData};
use solana_signer::Signer;

const PROGRAM: Address = Address::new_from_array([0x4d; 32]);

fn calendar_ix(admin: &Keypair) -> solana_instruction::Instruction {
    set_calendar_ix(&PROGRAM, &admin.pubkey(), 0, &[1, 2]).unwrap()
}

#[test]
fn a_mock_cluster_signs_sends_and_reads() {
    let admin = Keypair::new();
    let mut ok = Cluster::with_client(RpcClient::new_mock("succeeds"));
    let signature = submit(&mut ok, &[calendar_ix(&admin)], &admin).unwrap();
    // A base58 ed25519 signature: 64 bytes are 86 to 88 characters.
    assert!((86..=88).contains(&signature.len()), "{signature}");
    assert_eq!(ok.account(&PROGRAM).unwrap(), None);

    // An account as getAccountInfo returns it (base64 data).
    let owner = Address::new_from_array([7; 32]);
    let data = vec![1u8, 2, 3, 250];
    let mut mocks = HashMap::new();
    mocks.insert(
        RpcRequest::GetAccountInfo,
        json!({"context": {"slot": 1}, "value": {
            "lamports": 1, "owner": owner.to_string(), "executable": false, "rentEpoch": 0,
            "space": 4, "data": [base64::engine::general_purpose::STANDARD.encode(&data), "base64"]
        }}),
    );
    let mut with_account = Cluster::with_client(RpcClient::new_mock_with_mocks("succeeds", mocks));
    let info = with_account.account(&PROGRAM).unwrap().unwrap();
    assert_eq!(info.owner, owner);
    assert_eq!(info.data, data);
}

#[test]
fn a_failed_transaction_is_an_rpc_error() {
    let admin = Keypair::new();
    let mut failing = Cluster::with_client(RpcClient::new_mock("instruction_error"));
    match submit(&mut failing, &[calendar_ix(&admin)], &admin) {
        Err(CliError::Rpc(message)) => assert!(
            message.contains("instruction requires an initialized account"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn preflight_failures_carry_the_program_logs() {
    let simulation = serde_json::from_value(json!({
        "err": null,
        "logs": ["Program 4d invoke [1]", "Program 4d failed: custom program error: 0x6c"],
    }))
    .unwrap();
    let e = ClientError::from(ErrorKind::RpcError(RpcError::RpcResponseError {
        code: -32002,
        message: "Transaction simulation failed: custom program error: 0x6c".into(),
        data: RpcResponseErrorData::SendTransactionPreflightFailure(simulation),
    }));
    let text = client_error(&e, "https://rpc.example").to_string();
    assert!(text.starts_with("RPC: RPC response error -32002"), "{text}");
    assert!(
        text.ends_with(
            "\n  Program 4d invoke [1]\n  Program 4d failed: custom program error: 0x6c"
        ),
        "{text}"
    );
}

#[test]
fn urls_are_redacted_in_any_form() {
    let url = "https://rpc.example?api-key=SECRET";
    for text in [
        "error sending request for url (https://rpc.example/?api-key=SECRET)",
        "error sending request for url (https://rpc.example?api-key=SECRET): refused",
        "https://rpc.example?api-key=SECRET",
        "see \"http://other.example/SECRET\" and 'https://x/SECRET'",
    ] {
        let clean = redact(text, url);
        assert!(!clean.contains("SECRET"), "{clean}");
        assert!(clean.contains("<rpc url>"), "{clean}");
    }
    assert_eq!(
        redact(
            "error sending request for url (https://a/?k=SECRET): refused",
            ""
        ),
        "error sending request for url (<rpc url>): refused"
    );
    assert_eq!(redact("no url here", "https://a"), "no url here");
}

#[test]
fn a_real_transport_error_hides_the_url() {
    // Port 9 on the loopback refuses the connection at once; no request leaves the machine.
    let url = "http://127.0.0.1:9/?api-key=SECRET";
    let mut cluster = Cluster::new(url);
    let err = cluster.latest_blockhash().unwrap_err();
    let text = format!("{err} {err:?} {cluster:?}");
    assert!(!text.contains("SECRET"), "{text}");
    assert!(!text.contains("127.0.0.1"), "{text}");
    assert!(text.contains("<rpc url>"), "{text}");
}

#[test]
fn the_devnet_parameters_file_matches_the_runbook() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/devnet-params.json");
    let text = std::fs::read_to_string(path).unwrap();
    let params = ParamsFile::from_json(&text).unwrap().to_params().unwrap();
    assert_eq!(params.encode().len(), 161);
    assert_eq!(params.strike_tick, [5_000_000, 250_000]);
    assert_eq!(params.multiplier, [1_000, 50_000]);
    assert_eq!(params.expo, [-3, -5]);
    assert_eq!(params.receiver_program, metalbook_sdk::PYTH_RECEIVER);
    assert_eq!(params.challenge_delay_s, 600);
    assert_eq!(params.feed_id[0][0], 0x76);
    assert_eq!(params.feed_id[1][0], 0xf2);
    // Unknown and missing fields are rejected rather than defaulted.
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["typo_s"] = json!(1);
    assert!(ParamsFile::from_json(&value.to_string()).is_err());
    value.as_object_mut().unwrap().remove("typo_s");
    value.as_object_mut().unwrap().remove("slot_s");
    assert!(ParamsFile::from_json(&value.to_string()).is_err());
}

#[test]
fn calendar_lists_are_checked_before_signing() {
    let admin = Address::new_from_array([1; 32]);
    assert!(set_calendar_ix(&PROGRAM, &admin, 0, &[2, 1]).is_err());
    assert!(set_calendar_ix(&PROGRAM, &admin, 0, &[1, 1]).is_err());
    let too_many: Vec<i64> = (1..=65).collect();
    assert!(set_calendar_ix(&PROGRAM, &admin, 0, &too_many).is_err());
    assert!(set_calendar_ix(&PROGRAM, &admin, 0, &too_many[..64]).is_ok());
    assert!(set_calendar_ix(&PROGRAM, &admin, 0, &[]).is_ok());
}
