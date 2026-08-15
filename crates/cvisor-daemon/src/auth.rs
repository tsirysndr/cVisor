//! Bearer-token auth shared by the gRPC and GraphQL front-ends.
//!
//! The token comes from `CVISOR_TOKEN` if set, otherwise one is generated at
//! startup and printed to the console (see `main`). Clients present it as
//! `authorization: Bearer <token>` (gRPC metadata / HTTP header / GraphQL-WS
//! connection params).

use std::sync::Arc;

pub struct Auth {
    token: String,
    generated: bool,
}

impl Auth {
    /// Resolve the token: `CVISOR_TOKEN` if non-empty, else a fresh random one.
    pub fn resolve() -> Arc<Auth> {
        match std::env::var("CVISOR_TOKEN") {
            Ok(t) if !t.is_empty() => Arc::new(Auth {
                token: t,
                generated: false,
            }),
            _ => Arc::new(Auth {
                token: generate(),
                generated: true,
            }),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// True if the token was auto-generated (so it should be printed on start).
    pub fn generated(&self) -> bool {
        self.generated
    }

    /// Check a presented token (already stripped of the `Bearer ` prefix).
    pub fn check(&self, presented: Option<&str>) -> bool {
        presented.is_some_and(|t| constant_time_eq(t.as_bytes(), self.token.as_bytes()))
    }

    /// Check a raw `authorization` header value (`Bearer <token>`).
    pub fn check_header(&self, header: Option<&str>) -> bool {
        self.check(header.and_then(strip_bearer))
    }
}

fn generate() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("getrandom failed");
    hex::encode(b)
}

/// Strip a case-insensitive `Bearer ` prefix.
pub fn strip_bearer(header: &str) -> Option<&str> {
    let (scheme, rest) = header.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then_some(rest.trim())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
