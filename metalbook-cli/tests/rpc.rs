//! The JSON-RPC client against a scripted HTTP transport (no network): the requests it sends,
//! how it reads answers, confirmation polling, error mapping, and that no error carries the URL.
//! Also the committed devnet parameters file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_macros,
    missing_docs
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine;
use metalbook_cli::params::ParamsFile;
use metalbook_cli::rpc::{HttpPost, JsonRpc};
use metalbook_cli::{set_calendar_ix, submit, CliError, Rpc};
use metalbook_sdk::Address;
use serde_json::{json, Value};
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction::Transaction;

/// A URL carrying a provider key, which must never appear in an error.
const URL: &str = "https://rpc.example/?api-key=SECRET-KEY";

/// Answers each POST with the next scripted `(status, body)` and records the requests.
#[derive(Default)]
struct Scripted {
    answers: RefCell<VecDeque<(u16, String)>>,
    requests: RefCell<Vec<(String, Value)>>,
}

impl Scripted {
    fn new(answers: &[(u16, Value)]) -> Self {
        Self {
            answers: RefCell::new(answers.iter().map(|(s, b)| (*s, b.to_string())).collect()),
            requests: RefCell::default(),
        }
    }
}

impl HttpPost for &Scripted {
    fn post_json(&self, url: &str, body: &str) -> Result<(u16, String), CliError> {
        self.requests
            .borrow_mut()
            .push((url.to_owned(), serde_json::from_str(body).unwrap()));
        self.answers
            .borrow_mut()
            .pop_front()
            .ok_or(CliError::Transport("connection failed"))
    }
}

fn ok(result: Value) -> (u16, Value) {
    (200, json!({"jsonrpc": "2.0", "id": 1, "result": result}))
}

fn client(http: &Scripted) -> JsonRpc<&Scripted> {
    JsonRpc::new(http, URL).with_polling(3, Duration::ZERO)
}

const BLOCKHASH: &str = "4sGjMW1sUnHzSxGspuhpqLDx6wiyjNtZAMdL4VZHirAn";

fn blockhash_answer() -> (u16, Value) {
    ok(
        json!({"context": {"slot": 1}, "value": {"blockhash": BLOCKHASH, "lastValidBlockHeight": 9}}),
    )
}

fn statuses(status: Value) -> (u16, Value) {
    ok(json!({"context": {"slot": 1}, "value": [status]}))
}

#[test]
fn latest_blockhash_request_and_answer() {
    let http = Scripted::new(&[blockhash_answer()]);
    let hash = client(&http).latest_blockhash().unwrap();
    assert_eq!(hash.to_string(), BLOCKHASH);
    let requests = http.requests.borrow();
    let (url, body) = &requests[0];
    assert_eq!(url, URL);
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["method"], "getLatestBlockhash");
    assert_eq!(body["params"][0]["commitment"], "confirmed");
}

#[test]
fn account_reads_missing_and_present() {
    let owner = Address::new_from_array([7; 32]);
    let data = vec![1u8, 2, 3, 250];
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    let http = Scripted::new(&[
        ok(json!({"context": {"slot": 1}, "value": null})),
        ok(json!({"context": {"slot": 1}, "value": {
            "owner": owner.to_string(), "data": [encoded, "base64"],
            "lamports": 1, "executable": false, "rentEpoch": 0, "space": 4
        }})),
    ]);
    let mut rpc = client(&http);
    let address = Address::new_from_array([9; 32]);
    assert_eq!(rpc.account(&address).unwrap(), None);
    let info = rpc.account(&address).unwrap().unwrap();
    assert_eq!(info.owner, owner);
    assert_eq!(info.data, data);
    let requests = http.requests.borrow();
    assert_eq!(requests[0].1["method"], "getAccountInfo");
    assert_eq!(requests[0].1["params"][0], address.to_string());
    assert_eq!(requests[0].1["params"][1]["encoding"], "base64");
}

/// A signed transaction through `submit`, with the scripted status answers after the send.
fn submit_with(status_answers: &[(u16, Value)]) -> (Result<String, CliError>, Scripted, Keypair) {
    let admin = Keypair::new();
    let program = Address::new_from_array([0x4d; 32]);
    let instruction = set_calendar_ix(&program, &admin.pubkey(), 0, &[1, 2]).unwrap();
    let mut answers = vec![blockhash_answer(), ok(json!("SIGNATURE"))];
    answers.extend_from_slice(status_answers);
    let http = Scripted::new(&answers);
    let result = submit(&mut client(&http), &[instruction], &admin);
    (result, http, admin)
}

#[test]
fn send_posts_the_signed_transaction_and_waits_for_confirmation() {
    let (result, http, admin) = submit_with(&[
        statuses(Value::Null),
        statuses(
            json!({"slot": 2, "confirmations": 0, "err": null, "confirmationStatus": "processed"}),
        ),
        statuses(
            json!({"slot": 2, "confirmations": 1, "err": null, "confirmationStatus": "confirmed"}),
        ),
    ]);
    let signature = result.unwrap();
    let requests = http.requests.borrow();
    assert_eq!(requests.len(), 5, "blockhash, send and three status polls");
    let send = &requests[1].1;
    assert_eq!(send["method"], "sendTransaction");
    assert_eq!(send["params"][1]["encoding"], "base64");
    assert_eq!(send["params"][1]["preflightCommitment"], "confirmed");
    // The wire bytes are the signed legacy transaction: fee payer and only signer the admin,
    // the scripted blockhash, and the signature `submit` returned.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(send["params"][0].as_str().unwrap())
        .unwrap();
    let tx: Transaction = wincode::deserialize(&bytes).unwrap();
    assert_eq!(tx.message.account_keys[0], admin.pubkey());
    assert_eq!(tx.message.header.num_required_signatures, 1);
    assert_eq!(tx.message.recent_blockhash.to_string(), BLOCKHASH);
    assert_eq!(tx.signatures[0].to_string(), signature);
    assert!(tx.verify().is_ok());
    for poll in &requests[2..] {
        assert_eq!(poll.1["method"], "getSignatureStatuses");
        assert_eq!(poll.1["params"][0][0], "SIGNATURE");
    }
}

#[test]
fn a_failed_transaction_reports_its_error() {
    let (result, _, _) = submit_with(&[statuses(
        json!({"slot": 2, "confirmations": 1, "err": {"InstructionError": [0, {"Custom": 101}]}, "confirmationStatus": "confirmed"}),
    )]);
    match result {
        Err(CliError::TxFailed { signature, err }) => {
            assert_eq!(signature, "SIGNATURE");
            assert!(err.contains("Custom") && err.contains("101"), "{err}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unconfirmed_transaction_gives_up_after_the_budget() {
    let (result, http, _) = submit_with(&[
        statuses(Value::Null),
        statuses(Value::Null),
        statuses(Value::Null),
    ]);
    assert!(matches!(
        result,
        Err(CliError::NotConfirmed { polls: 3, .. })
    ));
    assert_eq!(http.requests.borrow().len(), 5);
}

#[test]
fn a_preflight_failure_carries_the_program_logs() {
    let (result, _, _) = submit_with(&[]);
    assert_eq!(
        result.unwrap_err().to_string(),
        "RPC transport error: connection failed"
    );

    let admin = Keypair::new();
    let program = Address::new_from_array([0x4d; 32]);
    let instruction = set_calendar_ix(&program, &admin.pubkey(), 0, &[1]).unwrap();
    let http = Scripted::new(&[
        blockhash_answer(),
        (
            200,
            json!({"jsonrpc": "2.0", "id": 2, "error": {
                "code": -32002,
                "message": "Transaction simulation failed: Error processing Instruction 0: custom program error: 0x65",
                "data": {"logs": ["Program 4d log line", "Program 4d failed: custom program error: 0x65"]}
            }}),
        ),
    ]);
    match submit(&mut client(&http), &[instruction], &admin) {
        Err(CliError::Rpc { code, message }) => {
            assert_eq!(code, -32002);
            assert!(message.contains("0x65"));
            assert!(message.contains("\n  Program 4d log line"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn errors_never_carry_the_url() {
    let cases: Vec<Vec<(u16, Value)>> = vec![
        vec![(500, json!({"error": URL}))],
        vec![(401, json!(URL))],
        vec![(200, json!({"no": "result"}))],
        vec![ok(json!({"value": {"blockhash": "not base58!"}}))],
        vec![],
    ];
    for answers in cases {
        let http = Scripted::new(&answers);
        let err = client(&http).latest_blockhash().unwrap_err();
        let text = format!("{err} {err:?}");
        assert!(!text.contains("SECRET"), "{text}");
        assert!(!text.contains("rpc.example"), "{text}");
    }
    let http = Scripted::new(&[(503, json!(null))]);
    assert!(matches!(
        client(&http).latest_blockhash(),
        Err(CliError::HttpStatus(503))
    ));
}

#[test]
fn the_devnet_parameters_file_matches_the_runbook() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/devnet-params.json");
    let file = ParamsFile::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    let params = file.to_params().unwrap();
    assert_eq!(params.encode().len(), 161);
    assert_eq!(params.strike_tick, [5_000_000, 250_000]);
    assert_eq!(params.multiplier, [1_000, 50_000]);
    assert_eq!(params.expo, [-3, -5]);
    assert_eq!(params.receiver_program, metalbook_sdk::PYTH_RECEIVER);
    assert_eq!(params.challenge_delay_s, 600);
    assert_eq!(params.feed_id[0][0], 0x76);
    assert_eq!(params.feed_id[1][0], 0xf2);
    // Unknown and missing fields are rejected rather than defaulted.
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    value["typo_s"] = json!(1);
    assert!(ParamsFile::from_json(&value.to_string()).is_err());
    value.as_object_mut().unwrap().remove("typo_s");
    value.as_object_mut().unwrap().remove("slot_s");
    assert!(ParamsFile::from_json(&value.to_string()).is_err());
}

#[test]
fn calendar_lists_are_checked_before_signing() {
    let program = Address::new_from_array([0x4d; 32]);
    let admin = Address::new_from_array([1; 32]);
    assert!(set_calendar_ix(&program, &admin, 0, &[2, 1]).is_err());
    assert!(set_calendar_ix(&program, &admin, 0, &[1, 1]).is_err());
    let too_many: Vec<i64> = (1..=65).collect();
    assert!(set_calendar_ix(&program, &admin, 0, &too_many).is_err());
    assert!(set_calendar_ix(&program, &admin, 0, &too_many[..64]).is_ok());
    assert!(set_calendar_ix(&program, &admin, 0, &[]).is_ok());
}
