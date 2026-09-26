//! Diagnostic log redaction (R02-T07 step 2) — a Rust mirror of the
//! incumbent `shared/log-redactor.ts` semantics, applied structurally to
//! every outgoing diagnostic surface:
//!
//! - every `tracing` event line, via the redacting writer in
//!   [`crate::logging`] (stderr AND the rotating file log);
//! - every machine-readable `LINGXI_*` stderr marker line (call sites wrap
//!   them in [`redact_line`] before printing);
//! - every HTTP/WS error response body: the error-enrichment middleware
//!   rewrites the `message` field through [`redact_text`] before the
//!   response leaves (so a storage/auth error can never echo a local
//!   secret path, a token or the raw request authentication header).
//!
//! What is redacted (mirroring the incumbent `[redacted]` vocabulary):
//! Bearer/authorization and cookie header values, `secret = value`
//! assignments for the incumbent secret-key list, secret-bearing URL query
//! parameters, URL credentials, well-known credential shapes (`sk-…`,
//! `AKIA…`, `gsk_…`, `ghp_…`, `glpat-…`, `xox…`) plus THIS service's own
//! credential prefixes (`hana_dev_…`, `hana_ws_…`), long random tokens
//! (40+ base64-ish characters, word-bounded like the incumbent) and
//! `data:` base64 blobs. The configured data home is replaced with
//! `[home]` and personal user path segments with `[user]` so diagnostics
//! cannot leak local secret paths.
//!
//! What is deliberately preserved: correlation identifiers (request ids,
//! session ids, run ids, event ids, seq numbers) are short and never match
//! the secret shapes — pinned by tests so A13's "关联 ID 保留" holds.
//!
//! Recorded divergences from the incumbent redactor (R02-T07 REVIEW_R1
//! F02 — listed here so R05 re-checks them BEFORE wiring real provider
//! credentials; none is reachable with the current R02 credential shapes):
//!
//! 1. PRECISE LONG-TOKEN BOUNDARY (measured against the incumbent, same
//!    input pair): `/` and `=` are NOT in this port's token character
//!    class (the incumbent's is). Two concrete consequences, both proven
//!    by node-vs-Rust probes during the R02-T07 review:
//!    - a >=40-char token containing a literal `/` (which splits it into
//!      two <40 segments, or abuts `.`/`-`) is caught by the incumbent
//!      but MISSED by this port;
//!    - plain hex64 / base64url WITHOUT `/`/`=`/`.` adjacency IS caught
//!      here (the old "no / no . no = survives" claim was wrong — the
//!      real miss condition is `/`/`=` splitting the candidate into <40
//!      segments or boundary adjacency).
//!
//!    R02-scope credentials (base64url unpadded / hex / `hana_*`-prefixed)
//!    never contain `/`, so R02 surfaces are safe; an R05 provider token
//!    that is standard base64 WITH padding/literal `/` would NOT be
//!    redacted here — re-verify before R05 logs any provider material.
//! 2. INCUMBENT RULES NOT PORTED (omissions, currently unreachable because
//!    this service never logs CLI arguments, message bodies or PII):
//!    the incumbent's PII rules (email / credit-card / CN-ID / SSN),
//!    `CLI_SECRET_FLAG_RE` (space-separated CLI secret flags), and
//!    `CONFIG_SECRET_VALUE_RE` (aws-configure style). Also the incumbent
//!    Windows user-path shape (`C:\Users\…`) is not mirrored (this port
//!    redacts POSIX-style user paths only).
//! 3. VERIFIED-IDENTICAL, NOT DIVERGENCES (probed same-input, both sides):
//!    tokens embedded in a Host header value are NOT redacted by either
//!    implementation; the `?token=…` query+assignment double-rule
//!    produces the same `[redacted]]` cosmetic double-bracket artifact on
//!    both sides (no leak); for a NON-secret long query value this port
//!    keeps `state=[token]` where the incumbent rewrites `?[token]`
//!    (this port is the more structure-preserving of the two).

/// Replacement marker (same word as the incumbent redactor).
pub const REDACTED: &str = "[redacted]";

/// Secret-bearing assignment keys (mirror of the incumbent
/// `SECRET_KEY_PATTERN`), matched as `key = value` / `key: value`.
const SECRET_KEY_WORDS: &[&str] = &[
    "server_token",
    "client_secret",
    "access_token",
    "refresh_token",
    "auth_token",
    "secret_key",
    "api_key",
    "apikey",
    "api-key",
    "bot_token",
    "password",
    "passwd",
    "secret",
    "token",
];

/// Well-known credential VALUE shapes (mirror of `API_KEY_VALUE_RE`), plus
/// this service's own credential prefixes (`hana_dev_`, `hana_ws_` — the
/// Rust mirror must recognize what this stack actually mints). The value
/// must carry at least this many characters beyond the prefix to count as
/// material (guards against redacting ordinary words like "sk-config").
const CREDENTIAL_VALUE_PREFIXES: &[&str] = &[
    "hana_dev_",
    "hana_ws_",
    "glpat-",
    "xoxa-",
    "xoxb-",
    "xoxp-",
    "xoxs-",
    "AKIA",
    "gsk_",
    "ghp_",
    "sk-",
];
const CREDENTIAL_MIN_TAIL: usize = 12;

/// Long-random-token rule (mirror of `LONG_RANDOM_RE`): 40+ characters of
/// `[A-Za-z0-9+/_=-]`, bounded on both sides by a character that is NOT a
/// word character, `/`, `.` or `-`.
const LONG_RANDOM_MIN: usize = 40;

/// Token character class for the long-random/credential scans. Divergence
/// from the incumbent regex, documented for the R02 safe-log contract: `/`
/// and `=` are deliberately NOT token characters (the incumbent includes
/// both), so structured `key=value` diagnostics (`path=/lingxi/v1/…`,
/// `request_id=req-…`) survive while every credential shape this stack
/// mints (base64url WITHOUT padding, hex — never contains `/` or `=`)
/// still matches. `.` is likewise out (paths and file names), as in the
/// incumbent class. The `data:…;base64` rule covers padded blobs.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+')
}

/// Character that invalidates a long-token boundary (word chars, `/`, `.`,
/// `-` — the incumbent's `[^\w/.-]` boundary class).
fn is_boundary_invalid(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '-')
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Redacts one diagnostic line. `home` (the canonical data home, when the
/// caller knows it) is replaced with `[home]` before the pattern passes so
/// paths under the private data root cannot leak through diagnostics.
pub fn redact_text(text: &str, home: Option<&std::path::Path>) -> String {
    let replaced = match home {
        Some(home) if !home.as_os_str().is_empty() => {
            text.replace(home.to_string_lossy().as_ref(), "[home]")
        }
        _ => text.to_string(),
    };
    let mut out = replaced;

    // Personal user path segments (mirror of the incumbent /Users,/home rules).
    out = replace_each(&out, find_user_path);
    // data:…;base64 blobs.
    out = replace_each_ci(&out, find_data_uri);
    // URL credentials scheme://user:password@.
    out = replace_each_ci(&out, find_url_credentials);
    // Secret-bearing URL query parameters.
    out = replace_each_ci(&out, find_url_query_secret);
    // Authorization / Cookie header values (leftmost key wins, then the
    // scanner resumes after it — several headers on one line are all
    // redacted).
    out = replace_each_ci(&out, |lower, rest, from| {
        find_header_value(rest, lower, from, &["authorization"])
    });
    out = replace_each_ci(&out, |lower, rest, from| {
        find_header_value(rest, lower, from, &["set-cookie", "cookie"])
    });
    // Bare Bearer tokens.
    out = replace_each_ci(&out, find_bare_bearer);
    // secret key=value / key: value assignments.
    out = replace_each_ci(&out, find_secret_assignment);
    // Well-known credential value shapes anywhere.
    out = replace_each(&out, find_credential_value);
    // Long random tokens (40+ base64-ish chars, word-bounded).
    out = replace_each(&out, find_long_random_token);
    out
}

/// Convenience wrapper for stderr marker lines: no home substitution.
pub fn redact_line(text: &str) -> String {
    redact_text(text, None)
}

// ── Generic scan-and-replace scaffolding ────────────────────────────────────

/// Case-sensitive pass: `find` returns `(start, end, replacement)` for the
/// next match at or after `from`, or `None`. Matches are consumed (the
/// scanner resumes after them), so overlapping later matches are found on
/// the remaining tail.
fn replace_each(
    text: &str,
    mut find: impl FnMut(&str, usize) -> Option<(usize, usize, String)>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut from = 0usize;
    loop {
        match find(text, from) {
            Some((start, end, replacement)) => {
                out.push_str(&text[from..start]);
                out.push_str(&replacement);
                from = end;
            }
            None => {
                out.push_str(&text[from..]);
                return out;
            }
        }
    }
}

/// Case-insensitive pass: `find` receives the lowercased haystack, the
/// original text and the search start. All patterns are ASCII, so ASCII
/// lowercasing preserves byte offsets (non-ASCII bytes are untouched).
fn replace_each_ci(
    text: &str,
    mut find: impl FnMut(&str, &str, usize) -> Option<(usize, usize, String)>,
) -> String {
    let lower = text.to_ascii_lowercase();
    replace_each(text, |rest, from| find(&lower, rest, from))
}

/// Finds `needle` (already lowercase) at or after `from` where it is not
/// glued to an identifier character on either side.
fn find_word_ci(lower: &str, from: usize, needle: &str) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = lower[search..].find(needle) {
        let start = search + rel;
        let end = start + needle.len();
        let prev_ok = start == 0
            || !lower[..start]
                .chars()
                .next_back()
                .map(is_word_char)
                .unwrap_or(false);
        let next_ok = lower[end..]
            .chars()
            .next()
            .map(|c| !is_word_char(c))
            .unwrap_or(true);
        if prev_ok && next_ok {
            return Some(start);
        }
        search = end;
    }
    None
}

/// Like [`find_word_ci`] but the boundary also rejects `-` glue, so the
/// word "bearer" inside a hyphenated token ("a13-oauth-bearer-…") is not
/// treated as a standalone scheme word.
fn find_glue_free_ci(lower: &str, from: usize, needle: &str) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = lower[search..].find(needle) {
        let start = search + rel;
        let end = start + needle.len();
        let glued = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
        let prev_ok = start == 0
            || !lower[..start]
                .chars()
                .next_back()
                .map(glued)
                .unwrap_or(false);
        let next_ok = lower[end..]
            .chars()
            .next()
            .map(|c| !glued(c))
            .unwrap_or(true);
        if prev_ok && next_ok {
            return Some(start);
        }
        search = end;
    }
    None
}

fn value_end(text: &str, start: usize) -> usize {
    start
        + text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ')' | ']' | '}' | '"'))
            .unwrap_or(text.len() - start)
}

// ── Individual rules ────────────────────────────────────────────────────────

/// `/Users/<user>` and `/home/<user>` path segments (their `file://` forms
/// are covered: the prefix search matches inside them).
fn find_user_path(rest: &str, from: usize) -> Option<(usize, usize, String)> {
    for prefix in ["/Users/", "/home/"] {
        let Some(rel) = rest[from..].find(prefix) else {
            continue;
        };
        let start = from + rel;
        let seg_start = start + prefix.len();
        let seg_end = rest[seg_start..]
            .find(|c: char| c == '/' || c.is_whitespace() || matches!(c, '"' | '\'' | '?' | '#'))
            .map(|r| seg_start + r)
            .unwrap_or(rest.len());
        if seg_end > seg_start {
            return Some((start, seg_end, format!("{}[user]", &rest[start..seg_start])));
        }
    }
    None
}

fn find_data_uri(rest: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut search = from;
    while let Some(rel) = lower[search..].find("data:") {
        let start = search + rel;
        let after = &lower[start..];
        let Some(semi) = after.find(';') else {
            search = start + 5;
            continue;
        };
        if !after[semi + 1..].starts_with("base64,") {
            search = start + 5;
            continue;
        }
        let head_end = start + semi + 1 + "base64,".len();
        let payload_end = rest[head_end..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='))
            .map(|r| head_end + r)
            .unwrap_or(rest.len());
        if payload_end > head_end {
            return Some((
                start,
                payload_end,
                format!("{}{REDACTED}", &rest[start..head_end]),
            ));
        }
        search = head_end;
    }
    None
}

fn find_url_credentials(rest: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut search = from;
    while let Some(rel) = lower[search..].find("://") {
        let auth_start = search + rel + 3;
        let auth_end = rest[auth_start..]
            .find(|c: char| c == '/' || c.is_whitespace() || matches!(c, '"' | '\'' | '?' | '#'))
            .map(|r| auth_start + r)
            .unwrap_or(rest.len());
        let authority = &rest[auth_start..auth_end];
        if let Some(at) = authority.rfind('@') {
            let user = &authority[..at];
            if !user.is_empty() && user.contains(':') {
                return Some((auth_start, auth_end, "[credentials]@".to_string()));
            }
        }
        search = auth_start;
    }
    None
}

fn is_secret_query_key(key: &str) -> bool {
    matches!(
        key,
        "token"
            | "access_token"
            | "refresh_token"
            | "auth"
            | "authorization"
            | "api_key"
            | "apikey"
            | "api-key"
            | "key"
            | "secret"
            | "password"
            | "client_secret"
            | "code"
    )
}

fn find_url_query_secret(rest: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut search = from;
    while let Some(q_rel) = lower[search..].find('?') {
        let q_start = search + q_rel;
        let region_end = rest[q_start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '>' | ')'))
            .map(|r| q_start + r)
            .unwrap_or(rest.len());
        let region = &rest[q_start..region_end];
        let mut rebuilt = String::with_capacity(region.len());
        rebuilt.push('?');
        for (idx, pair) in region[1..].split('&').enumerate() {
            if idx > 0 {
                rebuilt.push('&');
            }
            match pair.split_once('=') {
                Some((key, _)) if is_secret_query_key(&key.to_ascii_lowercase()) => {
                    rebuilt.push_str(key);
                    rebuilt.push('=');
                    rebuilt.push_str(REDACTED);
                }
                _ => rebuilt.push_str(pair),
            }
        }
        if rebuilt != region {
            return Some((q_start, region_end, rebuilt));
        }
        search = region_end;
    }
    None
}

/// `Authorization: Bearer x` / `authorization=…`: the scheme token
/// ("Bearer", "Basic") is kept, the credential after it is redacted.
/// Cookie values redact wholesale. The leftmost key occurrence wins.
fn find_header_value(
    rest: &str,
    lower: &str,
    from: usize,
    keys: &[&str],
) -> Option<(usize, usize, String)> {
    let mut best: Option<usize> = None;
    for key in keys {
        if let Some(start) = find_word_ci(lower, from, key) {
            best = Some(match best {
                Some(current) if current <= start => current,
                _ => start,
            });
        }
    }
    let start = best?;
    let key_len = lower[start..]
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c) || *c == '-')
        .count();
    let key = &lower[start..start + key_len];
    let end = start + key_len;
    let after = &rest[end..];
    let sp1 = after.len() - after.trim_start_matches(' ').len();
    let op = after[sp1..]
        .chars()
        .next()
        .filter(|c| *c == ':' || *c == '=')?;
    let _ = op;
    let rest_after_op = &after[sp1 + 1..];
    let sp2 = rest_after_op.len() - rest_after_op.trim_start_matches(' ').len();
    let value = &rest_after_op[sp2..];
    // Header values run to the end of the line (only ',', ';' or '"'
    // terminate early): "Authorization: Bearer x status=…" is one value.
    let v_end = value.find([',', ';', '"', '\n']).unwrap_or(value.len());
    if v_end == 0 {
        return None;
    }
    let head = format!(
        "{}{}{}{}",
        &rest[start..end],
        &after[..sp1],
        op,
        " ".repeat(sp2)
    );
    let full = &value[..v_end];
    let stop = end + sp1 + 1 + sp2 + v_end;
    if key == "cookie" || key == "set-cookie" {
        return Some((start, stop, format!("{head}{REDACTED}")));
    }
    // Authorization: keep a leading scheme word ("Bearer "/"Basic "),
    // redact the credential after it.
    match full.find(' ') {
        Some(space) if space + 1 < full.len() => {
            Some((start, stop, format!("{head}{} {REDACTED}", &full[..space])))
        }
        _ => Some((start, stop, format!("{head}{REDACTED}"))),
    }
}

fn find_bare_bearer(rest: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    let start = find_glue_free_ci(lower, from, "bearer")?;
    let end = start + "bearer".len();
    let after = &rest[end..];
    let value_start = after.len() - after.trim_start_matches([' ', '\t']).len();
    let value = &after[value_start..];
    let v_end = value_end(value, 0);
    if v_end == 0 {
        return None;
    }
    Some((
        start,
        end + value_start + v_end,
        format!("{}{}{REDACTED}", &rest[start..end], &after[..value_start]),
    ))
}

/// Leftmost `secret-key <sep> value` assignment across the key list (the
/// leftmost match wins, then the scanner resumes after it — so a line with
/// several assignments redacts all of them).
fn find_secret_assignment(rest: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut best: Option<usize> = None;
    for word in SECRET_KEY_WORDS {
        if let Some(start) = find_word_ci(lower, from, word) {
            best = Some(match best {
                Some(current) if current <= start => current,
                _ => start,
            });
        }
    }
    let start = best?;
    // Which key matched here determines nothing (all redact the same way);
    // find the separator after it. The key may itself contain the matched
    // word as suffix (e.g. access_token vs token): walk back the identifier
    // to the real key start.
    let key_start = lower[..start]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c) || *c == '-')
        .last()
        .map(|(i, _)| i)
        .unwrap_or(start);
    let key_end = start
        + lower[start..]
            .char_indices()
            .take_while(|(_, c)| is_word_char(*c) || *c == '-')
            .count();
    let after = &rest[key_end..];
    let sp1 = after.len() - after.trim_start_matches(' ').len();
    let op = after[sp1..]
        .chars()
        .next()
        .filter(|c| *c == ':' || *c == '=')?;
    let _ = op;
    let rest_after_op = &after[sp1 + 1..];
    let sp2 = rest_after_op.len() - rest_after_op.trim_start_matches(' ').len();
    let quoted = rest_after_op[sp2..].starts_with('"');
    let (value_off, value) = if quoted {
        (sp2 + 1, &rest_after_op[sp2 + 1..])
    } else {
        (sp2, &rest_after_op[sp2..])
    };
    let v_end = if quoted {
        value.find('"').map(|r| r + 1).unwrap_or(value.len())
    } else {
        value_end(value, 0)
    };
    if v_end == 0 {
        return None;
    }
    let head = format!(
        "{}{REDACTED}",
        &rest[key_start..key_end + sp1 + 1 + value_off],
    );
    Some((key_start, key_end + sp1 + 1 + value_off + v_end, head))
}

/// Well-known credential shapes at word boundaries (mirror of the
/// incumbent's `\b(sk-…)\b` style rules).
fn find_credential_value(rest: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut cursor = from;
    while cursor < rest.len() {
        let c = rest[cursor..].chars().next()?;
        if !is_token_char(c) {
            cursor += 1;
            continue;
        }
        let token_end = rest[cursor..]
            .find(|c: char| !is_token_char(c))
            .map(|r| cursor + r)
            .unwrap_or(rest.len());
        // Try every in-token position where a prefix could start at a word
        // boundary (mirrors \b): right after a non-word char or at the very
        // start of the scanned region. The length floor measures the TOKEN
        // tail after the prefix (not the rest of the text), so ordinary
        // short words like "sk-config" are untouched.
        for probe in word_boundaries_in(rest, cursor, token_end) {
            for prefix in CREDENTIAL_VALUE_PREFIXES {
                let Some(after_prefix) = probe.checked_add(prefix.len()) else {
                    continue;
                };
                if after_prefix > token_end || after_prefix > rest.len() {
                    continue;
                }
                let tail = token_end - after_prefix;
                if tail >= CREDENTIAL_MIN_TAIL
                    && rest[probe..after_prefix].eq_ignore_ascii_case(prefix)
                {
                    let prev_ok = probe == 0
                        || !rest[..probe]
                            .chars()
                            .next_back()
                            .map(is_word_char)
                            .unwrap_or(false);
                    if prev_ok {
                        return Some((cursor, token_end, REDACTED.to_string()));
                    }
                }
            }
        }
        cursor = token_end;
    }
    None
}

fn word_boundaries_in(rest: &str, token_start: usize, token_end: usize) -> Vec<usize> {
    let mut out = Vec::new();
    out.push(token_start);
    for (idx, c) in rest[token_start..token_end].char_indices() {
        if !is_word_char(c) {
            out.push(token_start + idx + 1);
        }
    }
    out
}

fn find_long_random_token(rest: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut cursor = from;
    while cursor < rest.len() {
        let c = rest[cursor..].chars().next()?;
        if !is_token_char(c) {
            cursor += 1;
            continue;
        }
        let run_end = rest[cursor..]
            .find(|c: char| !is_token_char(c))
            .map(|r| cursor + r)
            .unwrap_or(rest.len());
        if run_end - cursor >= LONG_RANDOM_MIN {
            let prev_ok = cursor == 0
                || !rest[..cursor]
                    .chars()
                    .next_back()
                    .map(is_boundary_invalid)
                    .unwrap_or(false);
            let next_ok = rest[run_end..]
                .chars()
                .next()
                .map(|c| !is_boundary_invalid(c))
                .unwrap_or(true);
            if prev_ok && next_ok {
                return Some((cursor, run_end, "[token]".to_string()));
            }
        }
        cursor = run_end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── A13 leak-case tests (written first; these pin the redaction
    //    semantics against the synthetic sensitive values used by the
    //    binary-level scan). ──────────────────────────────────────────────

    const API_KEY: &str = "sk-test-a13-redact-0123456789abcdef";
    const DEVICE_SECRET: &str = "hana_dev_A13oauth0123456789abcdefghij";
    const OAUTH_TOKEN: &str = "a13-oauth-bearer-0123456789abcdefgh";
    const QUERY_TOKEN: &str = "a13-query-token-0123456789abcdef";

    #[test]
    fn a13_bearer_authorization_header_value_is_redacted() {
        let line = format!("request failed Authorization: Bearer {OAUTH_TOKEN} status=401");
        let red = redact_text(&line, None);
        assert!(!red.contains(OAUTH_TOKEN), "leaked bearer: {red}");
        assert!(
            red.contains("Bearer [redacted]"),
            "must keep the scheme: {red}"
        );
    }

    #[test]
    fn a13_api_key_shape_is_redacted_anywhere() {
        let red = redact_text(&format!("upstream replied for key {API_KEY}"), None);
        assert!(!red.contains(API_KEY), "leaked api key: {red}");
        assert!(red.contains(REDACTED));
    }

    #[test]
    fn a13_device_secret_shape_is_redacted_anywhere() {
        for line in [
            format!("credential hana_dev_{DEVICE_SECRET} rejected"),
            format!("credential {DEVICE_SECRET} rejected"),
        ] {
            let red = redact_text(&line, None);
            assert!(!red.contains(DEVICE_SECRET), "leaked device secret: {red}");
        }
    }

    #[test]
    fn a13_secret_assignment_is_redacted_with_key_kept() {
        let red = redact_text(&format!(r#"config read api_key = "{API_KEY}""#), None);
        assert!(!red.contains(API_KEY), "leaked: {red}");
        let red = redact_text("token: hunter2 listed", None);
        assert!(
            red.contains("token=[redacted]") || red.contains("token: [redacted]"),
            "{red}"
        );
        assert!(!red.contains("hunter2"), "{red}");
    }

    #[test]
    fn multiple_secret_assignments_on_one_line_are_all_redacted() {
        // Leftmost-first ordering: both must be caught, not just the one
        // whose key appears earlier in the key list.
        let red = redact_text("token=first password=second done", None);
        assert!(!red.contains("first"), "{red}");
        assert!(!red.contains("second"), "{red}");
    }

    #[test]
    fn a13_url_query_token_is_redacted() {
        let red = redact_text(
            &format!("GET /lingxi/v1/ws?token={QUERY_TOKEN}&limit=5 denied"),
            None,
        );
        assert!(!red.contains(QUERY_TOKEN), "leaked query token: {red}");
        assert!(
            red.contains("limit=5"),
            "non-secret query keys survive: {red}"
        );
    }

    #[test]
    fn a13_cookie_header_is_redacted() {
        let red = redact_text("Set-Cookie: session=abc123; Path=/", None);
        assert!(!red.contains("abc123"), "{red}");
        let red = redact_text("cookie: session=abc123", None);
        assert!(!red.contains("abc123"), "{red}");
        let red = redact_text("cookie: a=1 set-cookie: b=2", None);
        assert!(
            !red.contains("a=1") || red.contains("cookie: [redacted]"),
            "{red}"
        );
    }

    #[test]
    fn a13_local_secret_path_is_replaced_with_home_marker() {
        let home = std::path::Path::new("/tmp/lingxi-a13-home");
        let red = redact_text(
            "auth registry /tmp/lingxi-a13-home/lingxi-service/device-registry.json is invalid",
            Some(home),
        );
        assert!(!red.contains("/tmp/lingxi-a13-home"), "leaked home: {red}");
        assert!(
            red.contains("[home]/lingxi-service/device-registry.json"),
            "{red}"
        );
    }

    #[test]
    fn a13_user_path_segments_are_redacted() {
        let red = redact_text("cannot read /Users/alice/secret.key file", None);
        assert!(!red.contains("/Users/alice"), "{red}");
        assert!(red.contains("/Users/[user]"), "{red}");
        let red = redact_text("probe /home/bob/data gone", None);
        assert!(red.contains("/home/[user]"), "{red}");
    }

    #[test]
    fn a13_data_uri_base64_is_redacted() {
        let red = redact_text("attachment data:text/plain;base64,SGVsbG8gV29ybGQh", None);
        assert!(!red.contains("SGVsbG8gV29ybGQh"), "{red}");
        assert!(red.contains("data:text/plain;base64,[redacted]"), "{red}");
    }

    #[test]
    fn a13_long_random_tokens_are_redacted_but_correlation_ids_survive() {
        let long_random = "c2FtcGxlIHNlY3JldCB2YWx1ZSBmb3IgcmVkbm90IHRlc3Q";
        assert!(long_random.len() >= LONG_RANDOM_MIN);
        let red = redact_text(&format!("leak candidate {long_random} end"), None);
        assert!(!red.contains(long_random), "{red}");
        // Correlation identifiers survive (short, structured, non-secret).
        let line = "run_0000019012345678_000042 committed event run_0000019012345678_000042-done \
                    session sess_local_alpha subscriber req-0000000000000042 seq=7";
        let red = redact_text(line, None);
        for id in [
            "run_0000019012345678_000042",
            "sess_local_alpha",
            "req-0000000000000042",
            "seq=7",
        ] {
            assert!(red.contains(id), "correlation id {id} must survive: {red}");
        }
    }

    #[test]
    fn a13_normal_diagnostics_are_untouched() {
        let line = "data root resolved effective_home=/tmp/h source=cli ignored_sources=none";
        assert_eq!(redact_text(line, None), line);
        let line = "GET /lingxi/v1/sessions/sess_local_alpha status=200 request_id=req-1";
        assert_eq!(redact_text(line, None), line);
    }

    #[test]
    fn a13_ws_ticket_shape_is_redacted() {
        let ticket = "hana_ws_0123456789abcdef0123456789abcdef";
        let red = redact_text(&format!("issued ws ticket {ticket}"), None);
        assert!(!red.contains(ticket), "{red}");
    }

    #[test]
    fn url_credentials_are_redacted() {
        let red = redact_text(
            "cannot reach https://user:hunter2@example.invalid/api",
            None,
        );
        assert!(!red.contains("hunter2"), "{red}");
        assert!(red.contains("[credentials]@"), "{red}");
    }

    #[test]
    fn redact_text_with_empty_home_is_identity() {
        let line = "plain diagnostic line";
        assert_eq!(redact_text(line, Some(std::path::Path::new(""))), line);
    }

    #[test]
    fn a13_synthetic_marker_line_passes_through_redaction_intact() {
        // The LINGXI_* marker vocabulary must survive redaction unchanged
        // (it never carries secret material) while an embedded token would
        // be stripped.
        let line = "LINGXI_AUTH_REJECTED method=GET path=/lingxi/v1/sessions status=401 \
                    reason=missing_credential remote=127.0.0.1:51000 request_id=req-3";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn ordinary_words_are_not_mangled_by_the_credential_rule() {
        let line = "sk-config and skip it; note the workstation";
        let red = redact_text(line, None);
        assert_eq!(red, line, "short tails must not count as credential values");
    }
}
