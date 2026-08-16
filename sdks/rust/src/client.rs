use std::fmt;

use serde::Deserialize;
use serde_json::Value;

/// Result alias for SDK operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors surfaced by the SDK.
#[derive(Debug)]
pub enum Error {
    /// Transport-level failure or a non-2xx HTTP status with no GraphQL body.
    Http(String),
    /// The daemon returned a non-empty `errors` array.
    GraphQL(String),
    /// A response (or base64 payload) failed to decode.
    Decode(String),
    /// The native runtime failed (Linux `Sandbox` only).
    Runtime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(m) => write!(f, "cvisor http error: {m}"),
            Error::GraphQL(m) => write!(f, "cvisor graphql error: {m}"),
            Error::Decode(m) => write!(f, "cvisor decode error: {m}"),
            Error::Runtime(m) => write!(f, "cvisor runtime error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// A thin, blocking client for the cVisor daemon's GraphQL endpoint.
#[derive(Clone)]
pub struct Client {
    url: String,
    token: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct GqlResponse {
    data: Option<Value>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Deserialize)]
struct GqlError {
    message: String,
}

impl Client {
    /// Build a client for the daemon at `url` (e.g.
    /// `http://127.0.0.1:8080/graphql`) authenticating with the bearer `token`.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Client {
            url: url.into(),
            token: token.into(),
            agent: ureq::agent(),
        }
    }

    /// Build a client with a custom [`ureq::Agent`] (timeouts, proxy, TLS...).
    pub fn with_agent(
        url: impl Into<String>,
        token: impl Into<String>,
        agent: ureq::Agent,
    ) -> Self {
        Client {
            url: url.into(),
            token: token.into(),
            agent,
        }
    }

    /// Run a GraphQL query document; returns the raw `data` value.
    pub fn query(&self, doc: &str, vars: Value) -> Result<Value> {
        self.execute(doc, vars)
    }

    /// Run a GraphQL mutation document; returns the raw `data` value.
    pub fn mutate(&self, doc: &str, vars: Value) -> Result<Value> {
        self.execute(doc, vars)
    }

    fn execute(&self, doc: &str, vars: Value) -> Result<Value> {
        let payload = serde_json::json!({ "query": doc, "variables": vars });
        let resp = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", self.token))
            .send_json(payload);

        let text = match resp {
            Ok(r) => r.into_string().map_err(|e| Error::Http(e.to_string()))?,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                if body.trim_start().starts_with('{') {
                    body
                } else {
                    return Err(Error::Http(format!("HTTP {code} from daemon")));
                }
            }
            Err(e) => return Err(Error::Http(e.to_string())),
        };

        let parsed: GqlResponse =
            serde_json::from_str(&text).map_err(|e| Error::Decode(e.to_string()))?;
        if let Some(errs) = parsed.errors {
            if !errs.is_empty() {
                let msg = errs
                    .into_iter()
                    .map(|e| e.message)
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(Error::GraphQL(msg));
            }
        }
        Ok(parsed.data.unwrap_or(Value::Null))
    }
}
