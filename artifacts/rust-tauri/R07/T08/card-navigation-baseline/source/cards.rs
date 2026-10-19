//! 会话互动卡片：本机授权、按用户隔离的瞬态有界LRU；历史渲染时重新登记。
use crate::{Principal, ServiceState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use lingxi_protocol::{InteractiveCardPut, InteractiveCardPutResponse};
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

pub const MAX_CODE_BYTES: usize = 512 * 1024;
const MAX_VARS_BYTES: usize = 16 * 1024;
const MAX_CARDS: usize = 256;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const CSP: &str = "default-src 'none'; sandbox allow-scripts; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; manifest-src 'none'; form-action 'none'; base-uri 'none'; navigate-to 'none'";

type CardKey = (String, String);
#[derive(Clone)]
struct Entry {
    code: String,
    vars_css: String,
}
impl Entry {
    fn bytes(&self) -> usize {
        self.code.len() + self.vars_css.len()
    }
}
#[derive(Default)]
struct Cache {
    entries: HashMap<CardKey, Entry>,
    order: VecDeque<CardKey>,
    bytes: usize,
}
#[derive(Default)]
pub struct CardCache {
    inner: Mutex<Cache>,
}
impl CardCache {
    fn put(&self, owner: &str, id: &str, entry: Entry) {
        let key = (owner.to_owned(), id.to_owned());
        let mut cache = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = cache.entries.remove(&key) {
            cache.bytes -= old.bytes();
        }
        cache.order.retain(|other| other != &key);
        cache.bytes += entry.bytes();
        cache.entries.insert(key.clone(), entry);
        cache.order.push_back(key);
        while cache.entries.len() > MAX_CARDS || cache.bytes > MAX_TOTAL_BYTES {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            if let Some(old) = cache.entries.remove(&oldest) {
                cache.bytes -= old.bytes();
            }
        }
    }
    fn get(&self, owner: &str, id: &str) -> Option<Entry> {
        let key = (owner.to_owned(), id.to_owned());
        let mut cache = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let entry = cache.entries.get(&key)?.clone();
        cache.order.retain(|other| other != &key);
        cache.order.push_back(key);
        Some(entry)
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn error(status: StatusCode, text: &str) -> Response {
    (status, Json(json!({"error":text}))).into_response()
}

pub async fn put_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
    body: Result<Json<InteractiveCardPut>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    if !valid_id(&id) {
        return error(StatusCode::BAD_REQUEST, "invalid cardId");
    }
    if body.code.is_empty() {
        return error(StatusCode::BAD_REQUEST, "code required");
    }
    if body.code.len() > MAX_CODE_BYTES {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "code too large");
    }
    if body.vars_css.len() > MAX_VARS_BYTES {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "varsCss too large");
    }
    if body.title.as_ref().is_some_and(|title| title.len() > 500) {
        return error(StatusCode::BAD_REQUEST, "invalid title");
    }
    let Some(owner) = principal
        .user_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::FORBIDDEN, "card owner missing");
    };
    state.cards().put(
        owner,
        &id,
        Entry {
            code: body.code,
            vars_css: body.vars_css,
        },
    );
    Json(InteractiveCardPutResponse { ok: true }).into_response()
}
pub async fn get_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    if !valid_id(&id) {
        return error(StatusCode::BAD_REQUEST, "invalid cardId");
    }
    let Some(owner) = principal
        .user_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::FORBIDDEN, "card owner missing");
    };
    let Some(entry) = state.cards().get(owner, &id) else {
        return error(StatusCode::NOT_FOUND, "card not found");
    };
    let safe_vars: String = entry
        .vars_css
        .chars()
        .filter(|c| *c != '<' && *c != '>')
        .collect();
    let html = format!("{HEAD}{safe_vars}{MIDDLE}{}{TAIL}", entry.code);
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CONTENT_SECURITY_POLICY, CSP),
        ],
        html,
    )
        .into_response()
}

const HEAD: &str = r###"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root {
"###;
const MIDDLE: &str = r###"
  --font-serif: 'EB Garamond', 'Noto Serif SC', 'Source Han Serif SC', 'Songti SC', 'STSong', serif;
  --font-ui: system-ui, -apple-system, sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, monospace;
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
html {
  background: var(--bg-card, #FBF7EE);
  color: var(--text, #2A2622);
  scrollbar-width: none;
  -ms-overflow-style: none;
}
html::-webkit-scrollbar,
body::-webkit-scrollbar { width: 0; height: 0; }
body { padding: 12px 16px; font-family: var(--font-serif); font-size: 14px; line-height: 1.65; }

h1 { font-size: 1.35rem; font-weight: 500; line-height: 1.25; margin: 0 0 0.6em; }
h2 { font-size: 1.1rem; font-weight: 500; line-height: 1.3; margin: 0.8em 0 0.4em; border-left: 2px solid var(--accent, #537D96); padding-left: 8px; }
h3 { font-size: 0.95rem; font-weight: 500; line-height: 1.35; margin: 0.6em 0 0.3em; }
p { margin: 0.4em 0; }
small { font-size: 0.75rem; color: var(--text-muted, #6B6158); }
strong { font-weight: 500; color: var(--accent, #537D96); }
a { color: var(--accent, #537D96); text-decoration: none; }
hr { border: none; border-top: 0.5px solid var(--border, #D8CFBE); margin: 0.8em 0; }

table { width: 100%; border-collapse: collapse; font-size: 0.85rem; margin: 0.6em 0; }
th { text-align: left; font-weight: 500; color: var(--text-light, #4A433C); padding: 6px 8px; border-bottom: 1px solid var(--border, #D8CFBE); }
td { padding: 5px 8px; border-bottom: 0.5px solid rgba(0,0,0,0.06); color: var(--text, #2A2622); }
tr:last-child td { border-bottom: none; }

ul, ol { padding-left: 18px; margin: 0.4em 0; }
li { margin: 0.15em 0; }
li::marker { color: var(--accent, #537D96); }

pre { background: var(--bg, #F5EFE4); border: 0.5px solid var(--border, #D8CFBE); border-radius: 4px; padding: 8px 12px; overflow-x: auto; }
code { font-family: var(--font-mono); font-size: 0.82rem; color: var(--text, #2A2622); }

blockquote { border-left: 2px solid var(--accent, #537D96); padding: 4px 0 4px 12px; color: var(--text-muted, #6B6158); font-style: italic; margin: 0.5em 0; }

svg { display: block; width: 100%; max-width: 100%; }
svg text { font-family: var(--font-serif); }

.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; }
</style>
</head>
<body>
"###;
const TAIL: &str = r###"
<script>
(function() {
  var NS = 'hana.card-resize';
  var last = 0;
  function report() {
    var h = document.documentElement.scrollHeight;
    if (h !== last) { last = h; window.parent.postMessage({ type: NS, height: h }, '*'); }
  }
  if (document.readyState === 'complete') report();
  else window.addEventListener('load', report);
  new ResizeObserver(function() { report(); }).observe(document.body);
  // 响应宿主 ping，覆盖 ResizeObserver 首报被漏掉的竞态
  window.addEventListener('message', function(e) {
    if (e.data === 'hana.card-ping') { last = 0; report(); }
  });
})();
</script>
</body>
</html>"###;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn users_are_isolated_and_read_refreshes_bounded_lru() {
        let cards = CardCache::default();
        cards.put(
            "a",
            "same",
            Entry {
                code: "A".into(),
                vars_css: String::new(),
            },
        );
        cards.put(
            "b",
            "same",
            Entry {
                code: "B".into(),
                vars_css: String::new(),
            },
        );
        assert_eq!(cards.get("a", "same").unwrap().code, "A");
        assert_eq!(cards.get("b", "same").unwrap().code, "B");
        assert!(cards.get("other", "same").is_none());
        for n in 0..254 {
            cards.put(
                "a",
                &format!("c_{n}"),
                Entry {
                    code: "x".into(),
                    vars_css: String::new(),
                },
            );
        }
        cards.get("a", "same").unwrap();
        cards.put(
            "a",
            "new",
            Entry {
                code: "x".into(),
                vars_css: String::new(),
            },
        );
        assert!(cards.get("b", "same").is_none());
        assert!(cards.get("a", "same").is_some());
        assert_eq!(cards.inner.lock().unwrap().entries.len(), MAX_CARDS);
    }
    #[test]
    fn total_bytes_are_bounded_even_before_entry_capacity() {
        let cards = CardCache::default();
        for n in 0..130 {
            cards.put(
                "a",
                &n.to_string(),
                Entry {
                    code: "x".repeat(MAX_CODE_BYTES),
                    vars_css: "v".repeat(MAX_VARS_BYTES),
                },
            );
        }
        let cache = cards.inner.lock().unwrap();
        assert!(cache.bytes <= MAX_TOTAL_BYTES);
        assert!(cache.entries.len() < MAX_CARDS);
        assert!(!cache.entries.contains_key(&("a".into(), "0".into())));
        assert!(cache.entries.contains_key(&("a".into(), "129".into())));
    }
}
