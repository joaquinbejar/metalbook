//! Tests of the Hermes price source with fixtures only (no live calls): parsing a response in
//! the shape of Hermes' `/v2/updates/price/{t}` (`apps/hermes/server/src/api/types.rs`), the
//! request it sends, status handling, and that over a synthetic tape served with Hermes'
//! "first update at or after t" semantics it answers exactly like the reference `TapeSource`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::disallowed_macros,
    missing_docs
)]

use std::cell::RefCell;

use metalbook_keeper::{
    parse_update, HermesConfig, HermesSource, HttpGet, HttpResponse, KeeperError, PriceSource,
    PrintMsg, TapeSource, HERMES_API_KEY_VAR,
};

const FEED: [u8; 32] = [
    0x76, 0x5d, 0x2b, 0xa9, 0x06, 0xdb, 0xc3, 0x2c, 0xa1, 0x7c, 0xc1, 0x1f, 0x53, 0x10, 0xa8, 0x9e,
    0x9e, 0xe1, 0xf6, 0x42, 0x05, 0x08, 0xc6, 0x38, 0x61, 0xf2, 0xf8, 0xba, 0x4e, 0xe3, 0x4b, 0xb2,
];

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn config() -> HermesConfig {
    HermesConfig::from_lookup(|n| (n == HERMES_API_KEY_VAR).then(|| "test-key".to_string()))
        .unwrap()
}

/// A recorded request: URL and headers.
type Request = (String, Vec<(String, String)>);

/// A fake Hermes over a tape: for `/v2/updates/price/{t}` it returns the first message (in
/// publication order) with `publish_time >= t`, as Hermes' `RequestTime::FirstAfter` does.
struct FakeHermes {
    tape: Vec<PrintMsg>,
    now: i64,
    status: Option<u16>,
    seen: RefCell<Vec<Request>>,
}

impl FakeHermes {
    fn new(tape: Vec<PrintMsg>) -> Self {
        Self {
            tape,
            now: i64::MAX,
            status: None,
            seen: RefCell::new(Vec::new()),
        }
    }
}

fn body(m: &PrintMsg) -> String {
    format!(
        r#"{{"binary":{{"encoding":"base64","data":["AAEC"]}},"parsed":[{{"id":"{}","price":{{"price":"{}","conf":"{}","expo":{},"publish_time":{}}},"ema_price":{{"price":"{}","conf":"{}","expo":{},"publish_time":{}}},"metadata":{{"slot":1,"proof_available_time":{},"prev_publish_time":{}}}}}]}}"#,
        hex(&FEED),
        m.price,
        m.conf,
        m.expo,
        m.publish_time,
        m.price,
        m.conf,
        m.expo,
        m.publish_time,
        m.publish_time,
        m.prev_publish_time
    )
}

impl HttpGet for FakeHermes {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        self.seen
            .borrow_mut()
            .push((url.to_string(), headers.to_vec()));
        if let Some(status) = self.status {
            return Ok(HttpResponse {
                status,
                body: String::new(),
            });
        }
        // Like Hermes, only what is already published (<= now) is served.
        let path = url
            .split("/v2/updates/price/")
            .nth(1)
            .unwrap()
            .split('?')
            .next()
            .unwrap();
        let found = if path == "latest" {
            self.tape.iter().rev().find(|m| m.publish_time <= self.now)
        } else {
            let t: i64 = path.parse().unwrap();
            self.tape
                .iter()
                .find(|m| m.publish_time >= t && m.publish_time <= self.now)
        };
        Ok(match found {
            Some(m) => HttpResponse {
                status: 200,
                body: body(m),
            },
            None => HttpResponse {
                status: 404,
                body: r#"{"error":"not found"}"#.into(),
            },
        })
    }
}

#[test]
fn test_parse_recorded_shape_fixture() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/hermes_xau.json"
    ))
    .unwrap();
    let u = parse_update(&json, &FEED).unwrap();
    assert_eq!(
        u.print,
        PrintMsg {
            price: 4_023_655,
            conf: 125,
            expo: -3,
            publish_time: 1_793_289_597,
            prev_publish_time: 1_793_289_596
        }
    );
    assert_eq!(
        u.update_data,
        b"PNAU\x01\x00\x00\x00\x03\xb8\x01\x00\x00\x00\x04\x0d".to_vec()
    );
    // Wrong feed, bad JSON, wrong encoding, non-integer price, missing prev_publish_time.
    assert!(matches!(
        parse_update(&json, &[0; 32]),
        Err(KeeperError::Source(_))
    ));
    assert!(matches!(
        parse_update("{", &FEED),
        Err(KeeperError::Source(_))
    ));
    assert!(matches!(
        parse_update(&json.replace("base64", "hex"), &FEED),
        Err(KeeperError::Source(_))
    ));
    assert!(matches!(
        parse_update(&json.replace("\"4023655\"", "\"4.0\""), &FEED),
        Err(KeeperError::Source(_))
    ));
    let no_prev = json.replace(", \"prev_publish_time\": 1793289596", "");
    assert_eq!(
        parse_update(&no_prev, &FEED)
            .unwrap()
            .print
            .prev_publish_time,
        0
    );
}

#[test]
fn test_request_shape_and_auth_header() {
    let fake = FakeHermes::new(vec![PrintMsg {
        price: 1,
        conf: 0,
        expo: -3,
        publish_time: 100,
        prev_publish_time: 99,
    }]);
    let source = HermesSource::new(config(), fake);
    source.first_at_or_after(&FEED, 100).unwrap().unwrap();
    // The source owns the fake; inspect through a second instance sharing nothing: rebuild.
    let fake = FakeHermes::new(vec![]);
    let source2 = HermesSource::new(config(), &fake);
    assert_eq!(source2.first_at_or_after(&FEED, 5).unwrap(), None);
    let (url, headers) = fake.seen.borrow()[0].clone();
    assert_eq!(url, format!("https://hermes.pyth.network/v2/updates/price/5?ids%5B%5D={}&encoding=base64&parsed=true", hex(&FEED)));
    assert_eq!(
        headers,
        vec![("Authorization".to_string(), "Bearer test-key".to_string())]
    );
    drop(source);
}

#[test]
fn test_status_handling() {
    for (status, ok) in [(401u16, false), (403, false), (500, false), (404, true)] {
        let mut fake = FakeHermes::new(vec![]);
        fake.status = Some(status);
        let r = HermesSource::new(config(), fake).first_at_or_after(&FEED, 1);
        assert_eq!(r.is_ok(), ok, "{status}: {r:?}");
        if status == 401 {
            assert!(format!("{r:?}").contains("unverified"));
        }
    }
}

/// The tape of the keeper tests' shape: a print every 1 to 3 seconds, a same-second repeat
/// every 7th step, failed aggregations, and a halt.
fn tape() -> Vec<PrintMsg> {
    let mut out = Vec::new();
    let (mut t, mut last_ok, mut px, mut step) = (10_000i64, 9_999i64, 4_000_000i64, 0u64);
    while t < 14_000 {
        step += 1;
        if (11_000..11_400).contains(&t) {
            t = 11_400;
            continue;
        }
        px += ((step * 7_919) % 41) as i64 - 20;
        out.push(PrintMsg {
            price: px,
            conf: 100,
            expo: -3,
            publish_time: t,
            prev_publish_time: last_ok,
        });
        if step % 7 == 0 {
            out.push(PrintMsg {
                price: px + 500,
                conf: 100,
                expo: -3,
                publish_time: t,
                prev_publish_time: t,
            });
        }
        if step % 11 == 0 {
            out.push(PrintMsg {
                price: px - 300,
                conf: 100,
                expo: -3,
                publish_time: last_ok,
                prev_publish_time: last_ok,
            });
        }
        last_ok = t;
        t += 1 + (step % 3) as i64;
    }
    out
}

impl HttpGet for &FakeHermes {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        (*self).get(url, headers)
    }
}

#[test]
fn test_hermes_source_answers_like_the_tape_reference() {
    let t = tape();
    let reference = TapeSource::new(FEED, t.clone());
    let now = 13_500;
    let mut fake = FakeHermes::new(t);
    fake.now = now;
    let hermes = HermesSource::new(config(), &fake);
    let mut checked = 0;
    for q in (9_990..13_600).step_by(7) {
        assert_eq!(
            hermes.canonical_at(&FEED, q, 10, now).unwrap(),
            reference.canonical_at(&FEED, q, 10, now).unwrap(),
            "canonical_at {q}"
        );
        assert_eq!(
            hermes.next_after(&FEED, q, now).unwrap(),
            reference.next_after(&FEED, q, now).unwrap(),
            "next_after {q}"
        );
        assert_eq!(
            hermes
                .latest_at_or_before(&FEED, q, q - 3_600, now)
                .unwrap(),
            reference
                .latest_at_or_before(&FEED, q, q - 3_600, now)
                .unwrap(),
            "latest_at_or_before {q}"
        );
        checked += 1;
    }
    assert!(checked > 400);
}

/// Before any update at or after `t` exists (Hermes answers 404), the latest fallback comes
/// from `/v2/updates/price/latest`, including when the latest update is a repeat.
#[test]
fn test_latest_fallback_when_nothing_after_t_is_published() {
    let t = tape();
    for now in [11_000, 11_200, 13_000] {
        let reference = TapeSource::new(FEED, t.clone());
        let mut fake = FakeHermes::new(t.clone());
        fake.now = now;
        let hermes = HermesSource::new(config(), &fake);
        let got = hermes
            .latest_at_or_before(&FEED, now + 50, now - 3_600, now)
            .unwrap();
        assert_eq!(
            got,
            reference
                .latest_at_or_before(&FEED, now + 50, now - 3_600, now)
                .unwrap(),
            "now {now}"
        );
        assert!(got.is_some());
        assert!(
            fake.seen
                .borrow()
                .iter()
                .any(|(u, _)| u.contains("/price/latest?")),
            "now {now}"
        );
    }
}
