//! Router and request handling.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::{self, Credentials};
use crate::config::Share;
use crate::listing::{self, Order, SortKey};
use crate::views::{self, Listing, Site};

pub struct App {
    pub title: String,
    pub theme: String,
    pub shares: Vec<Share>,
}

impl App {
    /// A single share without a name is served directly under `/` (quick mode).
    fn has_index(&self) -> bool {
        !self.shares.iter().any(|s| s.name.is_empty())
    }

    fn site(&self) -> Site<'_> {
        Site {
            title: &self.title,
            theme: &self.theme,
            has_index: self.has_index(),
        }
    }
}

pub fn router(app: App, creds: Option<Credentials>) -> Router {
    let router = Router::new()
        .route("/_rsrvdir/{file}", get(asset))
        .fallback(get(serve))
        .with_state(Arc::new(app));
    let router = match creds {
        Some(c) => router.layer(middleware::from_fn_with_state(Arc::new(c), auth::require)),
        None => router,
    };
    router.layer(middleware::from_fn(access_log))
}

async fn access_log(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let (method, uri) = (req.method().clone(), req.uri().clone());
    let res = next.run(req).await;
    tracing::info!(peer = %peer.ip(), %method, %uri, status = res.status().as_u16());
    res
}

async fn asset(Path(file): Path<String>) -> Response {
    let (body, mime): (&'static [u8], &str) = match file.as_str() {
        "style.css" => (
            include_bytes!("../assets/style.css"),
            "text/css; charset=utf-8",
        ),
        "open-props.min.css" => (
            include_bytes!("../assets/open-props.min.css"),
            "text/css; charset=utf-8",
        ),
        "alpine.min.js" => (
            include_bytes!("../assets/alpine.min.js"),
            "text/javascript; charset=utf-8",
        ),
        "icon.svg" => (include_bytes!("../assets/icon.svg"), "image/svg+xml"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        body,
    )
        .into_response()
}

/// RFC 5987 attr-char: everything else is percent-encoded in `filename*`.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

async fn serve(State(app): State<Arc<App>>, req: Request) -> Response {
    let raw_path = req.uri().path().to_owned();
    let Ok(path) = percent_decode_str(&raw_path).decode_utf8() else {
        return not_found();
    };
    let path = path.into_owned();

    if path == "/" && app.has_index() {
        return views::index(&app.site(), &app.shares).into_response();
    }

    // which share, and the rest of the path inside it
    let (share, rest) = match app.shares.iter().find(|s| s.name.is_empty()) {
        Some(s) => (s, &path[1..]),
        None => {
            let trimmed = &path[1..];
            let (name, rest) = trimmed.split_once('/').unwrap_or((trimmed, ""));
            let Some(s) = app.shares.iter().find(|s| s.name == name) else {
                return not_found();
            };
            if !trimmed.contains('/') {
                return Redirect::permanent(&format!("{raw_path}/")).into_response();
            }
            (s, rest)
        }
    };

    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segments
        .iter()
        .any(|s| *s == "." || *s == ".." || s.contains('\\') || s.contains('\0'))
    {
        return not_found();
    }
    let rel = segments.join("/");
    let full = share.root.join(&rel);
    // symlinks must not lead outside the share
    let Ok(real) = full.canonicalize() else {
        return not_found();
    };
    if !real.starts_with(&share.root) || (!rel.is_empty() && share.is_hidden(&rel)) {
        return not_found();
    }

    if real.is_dir() {
        if !raw_path.ends_with('/') {
            return Redirect::permanent(&format!("{raw_path}/")).into_response();
        }
        list_dir(&app, share, real, &rel, req.uri().query()).await
    } else {
        if !share.is_allowed_file(&rel) {
            return not_found();
        }
        // ?download forces an attachment (download button), otherwise per share
        let download = share.download
            || req
                .uri()
                .query()
                .is_some_and(|q| q.split('&').any(|kv| kv == "download"));
        send_file(share, download, &real, &rel, req).await
    }
}

async fn list_dir(
    app: &App,
    share: &Share,
    dir: PathBuf,
    rel: &str,
    query: Option<&str>,
) -> Response {
    let (mut sort, mut order) = (share.sort, share.order);
    for (k, v) in query
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
    {
        match k {
            "sort" => sort = SortKey::parse(v).unwrap_or(sort),
            "order" => order = Order::parse(v).unwrap_or(order),
            _ => {}
        }
    }
    let mut entries = match listing::read(share, &dir, rel) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "cannot read directory");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Cannot read directory").into_response();
        }
    };
    listing::sort(&mut entries, sort, order);
    let l = Listing {
        share,
        rel,
        entries: &entries,
        sort,
        order,
    };
    views::listing(&app.site(), &l).into_response()
}

async fn send_file(
    share: &Share,
    download: bool,
    file: &std::path::Path,
    rel: &str,
    req: Request,
) -> Response {
    let mut mime = mime_guess::from_path(file).first_or_octet_stream();
    if let (Some(cs), "text") = (&share.charset, mime.type_().as_str())
        && let Ok(m) = format!("{}; charset={cs}", mime.essence_str()).parse()
    {
        mime = m;
    }
    let mut res = match ServeFile::new_with_mime(file, &mime).oneshot(req).await {
        Ok(r) => r.map(Body::new),
        Err(e) => match e {},
    };
    if download && res.status().is_success() {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let ascii: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_graphic() && c != '"' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let value = format!(
            "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
            utf8_percent_encode(name, ATTR_CHAR)
        );
        if let Ok(v) = HeaderValue::from_str(&value) {
            res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
        }
    }
    res
}
