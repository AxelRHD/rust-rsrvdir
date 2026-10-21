//! Optional HTTP Basic Auth (inline `user:password` or an htpasswd file with bcrypt hashes).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;

/// user -> bcrypt hash
pub type Credentials = HashMap<String, String>;

pub fn inline(spec: &str) -> Result<Credentials> {
    let Some((user, pass)) = spec.split_once(':') else {
        bail!("invalid --auth format, expected user:password")
    };
    let hash = bcrypt::hash(pass, bcrypt::DEFAULT_COST)?;
    Ok(HashMap::from([(user.to_string(), hash)]))
}

pub fn parse_htpasswd(path: &Path) -> Result<Credentials> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut creds = Credentials::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((user, hash)) = line.split_once(':') else {
            bail!("{}:{}: expected user:hash", path.display(), i + 1)
        };
        if !hash.starts_with("$2") {
            bail!(
                "{}:{}: only bcrypt hashes are supported",
                path.display(),
                i + 1
            );
        }
        creds.insert(user.to_string(), hash.to_string());
    }
    Ok(creds)
}

/// Adds or replaces `user` in the htpasswd file (created if missing).
pub fn set_user(path: &Path, user: &str, password: &str) -> Result<()> {
    if user.contains(':') {
        bail!("user name must not contain ':'");
    }
    let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)?;
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| l.split_once(':').is_none_or(|(u, _)| u != user))
        .map(str::to_owned)
        .collect();
    lines.push(format!("{user}:{hash}"));
    std::fs::write(path, lines.join("\n") + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

pub async fn require(State(creds): State<Arc<Credentials>>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.split_once(':').map(|(u, p)| (u.to_owned(), p.to_owned())))
        .is_some_and(|(u, p)| {
            creds
                .get(&u)
                .is_some_and(|h| bcrypt::verify(p, h).unwrap_or(false))
        });
    if ok {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, r#"Basic realm="rsrvdir""#)],
        "Unauthorized",
    )
        .into_response()
}
