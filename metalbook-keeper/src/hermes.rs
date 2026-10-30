//! Hermes: configuration and the [`HermesSource`] price source.
//!
//! The API key is read only from the environment, never from a file and never printed: `Debug`
//! redacts it.
//!
//! Authentication: `Authorization: Bearer <key>`, the default, verified with a live call on
//! 2026-10-30 (every price endpoint answers 401 without a key). [`AuthScheme::Header`] sends the
//! key in a named header instead (`METALBOOK_HERMES_AUTH=header:<name>`) for gateways that
//! want one.
//!
//! Endpoint semantics, verified in the Hermes source (pyth-crosschain
//! `apps/hermes/server/src/api/rest/v2/timestamp_price_updates.rs`, `RequestTime::FirstAfter`):
//! `GET /v2/updates/price/{t}` returns, per feed, the **first** update with
//! `publish_time >= t`. Its parsed form carries `price` and `conf` as decimal strings, `expo`,
//! `publish_time` and `metadata.prev_publish_time` (`apps/hermes/server/src/api/types.rs`).

use std::fmt;

use base64::Engine;
use serde::Deserialize;

use crate::{KeeperError, PriceSource, PrintMsg};

/// Environment variable that holds the Hermes API key.
pub const HERMES_API_KEY_VAR: &str = "METALBOOK_HERMES_API_KEY";

/// Environment variable that overrides the Hermes base URL.
pub const HERMES_URL_VAR: &str = "METALBOOK_HERMES_URL";

/// Environment variable that selects the authentication scheme: `bearer` (default) or
/// `header:<name>`.
pub const HERMES_AUTH_VAR: &str = "METALBOOK_HERMES_AUTH";

/// Default Hermes endpoint.
pub const DEFAULT_HERMES_URL: &str = "https://hermes.pyth.network";

/// How the API key is sent (bearer verified live; see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthScheme {
    /// `Authorization: Bearer <key>` (default).
    Bearer,
    /// `<name>: <key>`.
    Header(String),
}

/// How to reach Hermes.
#[derive(Clone, PartialEq, Eq)]
pub struct HermesConfig {
    base_url: String,
    api_key: String,
    auth: AuthScheme,
}

impl HermesConfig {
    /// Reads the key from [`HERMES_API_KEY_VAR`], the URL from [`HERMES_URL_VAR`] (default
    /// [`DEFAULT_HERMES_URL`]) and the scheme from [`HERMES_AUTH_VAR`] (default bearer).
    ///
    /// # Errors
    ///
    /// `KeeperError::Config` if the key is missing or empty (Hermes price endpoints return 401
    /// without one), or the scheme is not `bearer` or `header:<name>`.
    pub fn from_env() -> Result<Self, KeeperError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// As [`HermesConfig::from_env`], with an explicit variable lookup (for tests).
    ///
    /// # Errors
    ///
    /// As [`HermesConfig::from_env`].
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, KeeperError> {
        let api_key = lookup(HERMES_API_KEY_VAR)
            .filter(|k| !k.trim().is_empty())
            .ok_or(KeeperError::Config("METALBOOK_HERMES_API_KEY is not set"))?;
        let base_url = lookup(HERMES_URL_VAR).unwrap_or_else(|| DEFAULT_HERMES_URL.to_string());
        let auth = match lookup(HERMES_AUTH_VAR).as_deref().map(str::trim) {
            None | Some("") | Some("bearer") => AuthScheme::Bearer,
            Some(other) => match other.strip_prefix("header:") {
                Some(name) if !name.trim().is_empty() => {
                    AuthScheme::Header(name.trim().to_string())
                }
                _ => {
                    return Err(KeeperError::Config(
                        "METALBOOK_HERMES_AUTH must be bearer or header:<name>",
                    ))
                }
            },
        };
        Ok(Self {
            base_url,
            api_key,
            auth,
        })
    }

    /// Base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The API key, for the request header only.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// The authentication scheme.
    #[must_use]
    pub fn auth(&self) -> &AuthScheme {
        &self.auth
    }

    /// The `(name, value)` header that carries the key.
    #[must_use]
    pub fn auth_header(&self) -> (String, String) {
        match &self.auth {
            AuthScheme::Bearer => (
                "Authorization".to_string(),
                format!("Bearer {}", self.api_key),
            ),
            AuthScheme::Header(name) => (name.clone(), self.api_key.clone()),
        }
    }
}

impl fmt::Debug for HermesConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesConfig")
            // A gateway URL can embed the key (`https://gateway.example/<KEY>`).
            .field("base_url", &scrub(&self.base_url, &self.api_key))
            .field("api_key", &"<redacted>")
            .field("auth", &self.auth)
            .finish()
    }
}

/// An HTTP response: status and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: String,
}

/// A blocking HTTP GET, so tests can replay fixtures instead of calling Hermes.
pub trait HttpGet {
    /// GETs `url` with `headers`. Any status is a response; only transport failures are errors.
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` on a transport failure.
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError>;
}

/// [`HttpGet`] over `ureq` (rustls, no native TLS).
#[derive(Debug)]
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    /// An agent that returns every status as a response, with a 10-second timeout.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build()
            .into();
        Self { agent }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpGet for UreqHttp {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, KeeperError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let mut response = request
            .call()
            .map_err(|e| KeeperError::Source(format!("GET failed: {e}")))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| KeeperError::Source(format!("reading body failed: {e}")))?;
        Ok(HttpResponse { status, body })
    }
}

/// One Hermes update for one feed: the parsed print and the raw accumulator update (what the
/// Pyth receiver posts, milestone 7b-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HermesUpdate {
    /// The parsed price message.
    pub print: PrintMsg,
    /// The binary accumulator update, decoded from base64.
    pub update_data: Vec<u8>,
}

#[derive(Deserialize)]
struct WirePriceUpdate {
    binary: WireBinary,
    parsed: Option<Vec<WireParsed>>,
}

#[derive(Deserialize)]
struct WireBinary {
    encoding: String,
    data: Vec<String>,
}

#[derive(Deserialize)]
struct WireParsed {
    id: String,
    price: WirePrice,
    metadata: WireMetadata,
}

#[derive(Deserialize)]
struct WirePrice {
    price: String,
    conf: String,
    expo: i32,
    publish_time: i64,
}

#[derive(Deserialize)]
struct WireMetadata {
    prev_publish_time: Option<i64>,
}

/// Parses a `/v2/updates/price/{t}?encoding=base64&parsed=true` body for `feed`.
///
/// # Errors
///
/// `KeeperError::MalformedResponse` (status, length and a fixed category, never the body) for
/// malformed JSON, a non-base64 encoding, a missing feed, or a price or confidence that is not
/// an integer. A missing `prev_publish_time` is read as 0, which `accept_print` rejects.
pub fn parse_update(body: &str, feed: &[u8; 32]) -> Result<HermesUpdate, KeeperError> {
    // No error quotes the response: serde's messages can echo field values (`invalid type:
    // string "…"`), so every failure is a fixed category with the status and length only.
    let bad = |what: &'static str| KeeperError::MalformedResponse {
        status: 200,
        byte_len: body.len(),
        what,
    };
    let wire: WirePriceUpdate = serde_json::from_str(body).map_err(|e| {
        bad(match e.classify() {
            serde_json::error::Category::Syntax => "not JSON",
            serde_json::error::Category::Eof => "truncated JSON",
            serde_json::error::Category::Data => "JSON of the wrong shape",
            serde_json::error::Category::Io => "unreadable body",
        })
    })?;
    if wire.binary.encoding != "base64" {
        return Err(bad("unexpected binary encoding"));
    }
    let want = hex(feed);
    let parsed = wire
        .parsed
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.id.trim_start_matches("0x").eq_ignore_ascii_case(&want))
        .ok_or_else(|| bad("requested feed missing"))?;
    let price = parsed
        .price
        .price
        .parse::<i64>()
        .map_err(|_| bad("price is not an integer"))?;
    let conf = parsed
        .price
        .conf
        .parse::<u64>()
        .map_err(|_| bad("confidence is not an integer"))?;
    let raw = wire
        .binary
        .data
        .first()
        .ok_or_else(|| bad("no binary update"))?;
    let update_data = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| bad("binary update is not valid base64"))?;
    Ok(HermesUpdate {
        print: PrintMsg {
            price,
            conf,
            expo: parsed.price.expo,
            publish_time: parsed.price.publish_time,
            prev_publish_time: parsed.metadata.prev_publish_time.unwrap_or(0),
        },
        update_data,
    })
}

/// Removes the API key from an error message, as a last line of defence: no error built in
/// this module quotes a response, but a transport error could carry text from elsewhere.
fn redact(e: KeeperError, key: &str) -> KeeperError {
    match e {
        KeeperError::Source(msg) => KeeperError::Source(scrub(&msg, key)),
        KeeperError::Chain(msg) => KeeperError::Chain(scrub(&msg, key)),
        other => other,
    }
}

/// `text` with every occurrence of `key` replaced (unchanged when the key is empty).
fn scrub(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        text.replace(key, "<redacted>")
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    use fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// A [`PriceSource`] backed by Hermes (`/v2/updates/price/{t}`, the first update with
/// `publish_time >= t`, and `/v2/updates/price/latest`). Requests per query: `canonical_at`
/// and `next_after` one; `latest_at_or_before` at most three (the update at or after `t`, or
/// `latest` when there is none yet, then one step back). Errors never carry response content,
/// and the API key is scrubbed from any error text.
#[derive(Debug)]
pub struct HermesSource<H: HttpGet> {
    config: HermesConfig,
    http: H,
    /// How long no request is sent after a 429.
    cooldown: std::time::Duration,
    /// Until when no request is sent: Hermes counts requests made inside its window against
    /// the key, so retrying (or another series asking) only extends it.
    cooling_until: std::cell::Cell<Option<std::time::Instant>>,
    /// Least time between two requests (zero: none).
    min_interval: std::time::Duration,
    /// When the last request was sent.
    last_sent: std::cell::Cell<Option<std::time::Instant>>,
    /// Updates already fetched, by query time and by publish time (see
    /// [`HermesSource::first_at_or_after`]).
    cache: std::cell::RefCell<UpdateCache>,
}

/// History does not change: once Hermes has an update at or after `t`, the first one is fixed.
/// So a found update answers its own query time and its publish time forever. The timestamp
/// endpoint allows about 10 requests a minute per key (measured 2026-10-30, then 429 with
/// `retry-after: 60`), and every series of a metal asks for the same canonical prints.
#[derive(Debug, Default)]
struct UpdateCache {
    by_query: std::collections::HashMap<([u8; 32], i64), HermesUpdate>,
    by_publish: std::collections::HashMap<([u8; 32], i64), HermesUpdate>,
    /// The latest update per feed and when it was fetched: reused for [`LATEST_TTL`], so every
    /// series of a keeper pass anchors on the same update (and the same step back).
    /// `None` when Hermes had nothing for the feed: cached too, so remembered misses never
    /// cost a `/latest` each (codex, #28).
    latest: std::collections::HashMap<[u8; 32], (std::time::Instant, Option<HermesUpdate>)>,
    /// Times Hermes had nothing at or after (404). Unlike a found update, a miss is not
    /// permanent: it holds only while the feed's latest update is still before that time
    /// (codex, #30: without it a recurring 429 had each pass re-ask the same empty slots).
    misses: std::collections::HashSet<([u8; 32], i64)>,
}

/// How long a `/latest` answer is reused.
const LATEST_TTL: std::time::Duration = std::time::Duration::from_secs(10);

impl<H: HttpGet> HermesSource<H> {
    fn request(&self, url: &str, feed: &[u8; 32]) -> Result<Option<HermesUpdate>, KeeperError> {
        self.request_unredacted(url, feed)
            .map_err(|e| redact(e, self.config.api_key()))
    }

    fn request_unredacted(
        &self,
        url: &str,
        feed: &[u8; 32],
    ) -> Result<Option<HermesUpdate>, KeeperError> {
        if let Some(until) = self.cooling_until.get() {
            if std::time::Instant::now() < until {
                return Err(KeeperError::Source(
                    "Hermes rate limit: cooling down, request not sent".into(),
                ));
            }
        }
        if let Some(at) = self.last_sent.get() {
            let wait = self.min_interval.saturating_sub(at.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        self.last_sent.set(Some(std::time::Instant::now()));
        let response = self.http.get(url, &[self.config.auth_header()])?;
        if response.status == 429 {
            self.cooling_until
                .set(std::time::Instant::now().checked_add(self.cooldown));
            return Err(KeeperError::Source(
                "Hermes returned HTTP 429 (rate limited); cooling down".into(),
            ));
        }
        match response.status {
            200 => parse_update(&response.body, feed).map(Some),
            404 => Ok(None),
            401 | 403 => Err(KeeperError::Source(format!(
                "Hermes rejected the API key ({}); check the key and METALBOOK_HERMES_AUTH",
                response.status
            ))),
            other => Err(KeeperError::Source(format!("Hermes returned HTTP {other}"))),
        }
    }

    /// A source using `config` over `http`.
    #[must_use]
    pub fn new(config: HermesConfig, http: H) -> Self {
        Self {
            config,
            http,
            // Past Hermes' `retry-after: 60`.
            cooldown: std::time::Duration::from_secs(61),
            cooling_until: std::cell::Cell::new(None),
            min_interval: std::time::Duration::ZERO,
            last_sent: std::cell::Cell::new(None),
            cache: std::cell::RefCell::default(),
        }
    }

    /// Replaces how long no request is sent after a 429 (default 61 seconds, past Hermes'
    /// one-minute window). Every request in that time fails at once without reaching Hermes,
    /// so a keeper pass over many series does not keep the key inside the window; what was
    /// fetched stays cached and the next pass resumes.
    #[must_use]
    pub fn with_cooldown(mut self, cooldown: std::time::Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    /// Paces requests: at least `interval` between two of them (default none). The keeper
    /// binary uses 10 seconds, at most 6 a minute, under the timestamp endpoint's limit of
    /// about 10 a minute per key.
    #[must_use]
    pub fn with_min_interval(mut self, interval: std::time::Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// The first update of `feed` with `publish_time >= t`, or `None` if Hermes has none yet
    /// (404).
    ///
    /// # Errors
    ///
    /// `KeeperError::Source` for a 401 or 403 (the key is missing, wrong or sent the wrong way:
    /// see the module note), any other non-success status, or a malformed body.
    pub fn first_at_or_after(
        &self,
        feed: &[u8; 32],
        t: i64,
    ) -> Result<Option<HermesUpdate>, KeeperError> {
        {
            let cache = self.cache.borrow();
            // An update published exactly at `t` is the first at or after `t`.
            if let Some(u) = cache
                .by_query
                .get(&(*feed, t))
                .or_else(|| cache.by_publish.get(&(*feed, t)))
            {
                return Ok(Some(u.clone()));
            }
        }
        // A known miss holds while the latest update (one `/latest` request per feed per
        // LATEST_TTL) is still before `t`.
        if self.cache.borrow().misses.contains(&(*feed, t)) {
            match self.latest(feed)? {
                // Nothing published at all, or nothing since `t`: still a miss.
                None => return Ok(None),
                Some(latest) if latest.publish_time < t => return Ok(None),
                Some(_) => {}
            }
        }
        let url = format!(
            "{}/v2/updates/price/{t}?ids%5B%5D={}&encoding=base64&parsed=true",
            self.config.base_url().trim_end_matches('/'),
            hex(feed)
        );
        let found = self.request(&url, feed)?;
        if found.is_none() {
            self.cache.borrow_mut().misses.insert((*feed, t));
        }
        if let Some(u) = &found {
            let mut cache = self.cache.borrow_mut();
            cache.misses.remove(&(*feed, t));
            cache.by_query.insert((*feed, t), u.clone());
            cache
                .by_publish
                .insert((*feed, u.print.publish_time), u.clone());
        }
        Ok(found)
    }

    /// The latest update of `feed` (`/v2/updates/price/latest`), or `None` if Hermes has none.
    ///
    /// # Errors
    ///
    /// As [`HermesSource::first_at_or_after`].
    pub fn latest(&self, feed: &[u8; 32]) -> Result<Option<PrintMsg>, KeeperError> {
        if let Some((at, u)) = self.cache.borrow().latest.get(feed) {
            if at.elapsed() < LATEST_TTL {
                return Ok(u.as_ref().map(|u| u.print));
            }
        }
        let url = format!(
            "{}/v2/updates/price/latest?ids%5B%5D={}&encoding=base64&parsed=true",
            self.config.base_url().trim_end_matches('/'),
            hex(feed)
        );
        let found = self.request(&url, feed)?;
        self.cache
            .borrow_mut()
            .latest
            .insert(*feed, (std::time::Instant::now(), found.clone()));
        if let Some(u) = &found {
            let mut cache = self.cache.borrow_mut();
            // Only the first update of its second may answer "first at or after its publish
            // time"; a same-second repeat must not.
            if u.print.prev_publish_time < u.print.publish_time {
                cache
                    .by_publish
                    .insert((*feed, u.print.publish_time), u.clone());
            }
        }
        Ok(found.map(|u| u.print))
    }

    fn published(
        &self,
        feed: &[u8; 32],
        t: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        // Nothing at or after a time still in the future can be published by `now`: no
        // request (the keeper walks every slot of the window, most of them future while the
        // window runs, and Hermes' timestamp endpoint is rate limited).
        if t > now {
            return Ok(None);
        }
        Ok(self
            .first_at_or_after(feed, t)?
            .map(|u| u.print)
            .filter(|m| m.publish_time <= now))
    }
}

impl<H: HttpGet> PriceSource for HermesSource<H> {
    fn canonical_at(
        &self,
        feed: &[u8; 32],
        t: i64,
        tolerance: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        let latest = t
            .checked_add(tolerance)
            .ok_or(KeeperError::Config("tolerance overflow"))?;
        // The first update at or after t is the canonical one when it is within tolerance.
        Ok(self.published(feed, t, now)?.filter(|m| {
            m.prev_publish_time > 0 && m.prev_publish_time < t && m.publish_time <= latest
        }))
    }

    fn next_after(
        &self,
        feed: &[u8; 32],
        t: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        let after = t
            .checked_add(1)
            .ok_or(KeeperError::Config("time overflow"))?;
        Ok(self
            .published(feed, after, now)?
            .filter(|m| m.prev_publish_time > 0 && m.prev_publish_time <= t))
    }

    fn latest_at_or_before(
        &self,
        feed: &[u8; 32],
        t: i64,
        earliest: i64,
        now: i64,
    ) -> Result<Option<PrintMsg>, KeeperError> {
        let t = t.min(now);
        // An anchor. The latest update, when it is not after t, is the latest at or before t
        // (nothing newer exists), and `/latest` is not under the timestamp endpoint's rate
        // limit; otherwise the first update at or after t.
        let latest = self.latest(feed)?.filter(|m| m.publish_time <= now);
        let anchor = match latest {
            Some(m) if m.publish_time <= t => m,
            _ => match self.published(feed, t, now)? {
                Some(m) => m,
                None => return Ok(None),
            },
        };
        let candidate = if anchor.publish_time <= t {
            if anchor.prev_publish_time < anchor.publish_time {
                Some(anchor)
            } else {
                // A repeat (same-second aggregate or failed aggregation): the first update at
                // its publish time is the first of that second.
                self.published(feed, anchor.publish_time, now)?
            }
        } else if anchor.prev_publish_time > 0 {
            // The anchor is after t: its prev_publish_time is the last successful aggregate
            // before it, and fetching that time returns it.
            self.published(feed, anchor.prev_publish_time, now)?
        } else {
            None
        };
        Ok(candidate.filter(|m| {
            m.prev_publish_time > 0
                && m.prev_publish_time < m.publish_time
                && m.publish_time >= earliest
                && m.publish_time <= t
        }))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    #[test]
    fn test_key_comes_from_the_environment_and_is_redacted() {
        let missing = HermesConfig::from_lookup(|_| None);
        assert!(matches!(missing, Err(KeeperError::Config(_))));
        let empty =
            HermesConfig::from_lookup(|n| (n == HERMES_API_KEY_VAR).then(|| " ".to_string()));
        assert!(matches!(empty, Err(KeeperError::Config(_))));
        let c =
            HermesConfig::from_lookup(|n| (n == HERMES_API_KEY_VAR).then(|| "s3cr3t".to_string()))
                .unwrap();
        assert_eq!((c.base_url(), c.api_key()), (DEFAULT_HERMES_URL, "s3cr3t"));
        assert_eq!(c.auth(), &AuthScheme::Bearer);
        assert_eq!(
            c.auth_header(),
            ("Authorization".to_string(), "Bearer s3cr3t".to_string())
        );
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cr3t") && shown.contains("<redacted>"));
    }

    #[test]
    fn test_a_key_embedded_in_the_gateway_url_is_redacted() {
        const KEY: &str = "SYNTHETIC-HERMES-KEY";
        const URL: &str = "https://gateway.example/SYNTHETIC-HERMES-KEY";
        let c = HermesConfig::from_lookup(|n| match n {
            HERMES_API_KEY_VAR => Some(KEY.to_string()),
            HERMES_URL_VAR => Some(URL.to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(c.base_url(), URL, "the URL itself is used unchanged");
        let shown = format!("{c:?}");
        assert!(!shown.contains(KEY), "{shown}");
        assert!(
            shown.contains("https://gateway.example/<redacted>"),
            "{shown}"
        );
        // A transport error quoting the request URL is scrubbed the same way.
        let e = redact(
            KeeperError::Source(format!("GET failed: {URL}/v2/updates/price/latest")),
            c.api_key(),
        );
        let shown = format!("{e} {e:?}");
        assert!(!shown.contains(KEY), "{shown}");
    }

    #[test]
    fn test_auth_scheme_from_the_environment() {
        let lookup = |auth: &'static str| {
            move |n: &str| match n {
                HERMES_API_KEY_VAR => Some("k".to_string()),
                HERMES_AUTH_VAR => Some(auth.to_string()),
                _ => None,
            }
        };
        let c = HermesConfig::from_lookup(lookup("header:x-api-key")).unwrap();
        assert_eq!(c.auth_header(), ("x-api-key".to_string(), "k".to_string()));
        assert_eq!(
            HermesConfig::from_lookup(lookup("bearer")).unwrap().auth(),
            &AuthScheme::Bearer
        );
        assert!(matches!(
            HermesConfig::from_lookup(lookup("basic")),
            Err(KeeperError::Config(_))
        ));
        assert!(matches!(
            HermesConfig::from_lookup(lookup("header:")),
            Err(KeeperError::Config(_))
        ));
    }
}
