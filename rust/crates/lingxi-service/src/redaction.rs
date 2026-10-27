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
//! 关联编号保留原值。宿主 RequestId 使用独立引号边界，避免字段名与随机编号
//! 合并成一个长 token；任意输入仍执行全部秘密识别，没有编号前缀豁免。
//!
//! Recorded divergences from the incumbent redactor (R02-T07 REVIEW_R1
//! F02 — listed here so R05 re-checks them BEFORE wiring real provider
//! credentials; none is reachable with the current R02 credential shapes):
//!
//! 1. PRECISE LONG-TOKEN BOUNDARY — **FIXED IN R05-T02**
//!    (RR-R05-REDACTION-BOUNDARY). The port's token character class now
//!    matches the incumbent's `[A-Za-z0-9+/_=-]`: `/` and `=` are token
//!    characters, so a standard-base64 token containing literal `/` or
//!    padding `=` can no longer escape the 40+ long-token rule by being
//!    split into sub-40 segments. (The pre-fix port excluded both —
//!    measured against the incumbent during the R02-T07 review; the
//!    R02-scope credential shapes never contained `/` so the gap was
//!    unreachable until R05's provider material.) Pinned by
//!    `r05_t02_base64_tokens_with_slash_and_padding_are_redacted`.
//!
//!    History of the investigation: the R05-T01 re-check found and fixed a
//!    REAL defect in the case-insensitive plumbing — the direct
//!    `replace_each_ci` passes handed the original-case text to the finders
//!    where the lowercased haystack belonged, so mixed-case KEYS
//!    (`apiKey`, `Password`, `TOKEN`) escaped the assignment/query/header-
//!    word scans (and redacted heads came out lowercased). Fixed at the
//!    call sites; pinned by `r05_secret_key_words_match_case_insensitively`
//!    and `r05_provider_api_key_shapes_are_redacted`.
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

/// Token character class for the long-random/credential scans. R05-T02
/// (RR-R05-REDACTION-BOUNDARY) closed the divergence from the incumbent
/// regex: `/` and `=` ARE token characters now (the incumbent class is
/// `[A-Za-z0-9+/_=-]`), so a standard-base64 token with literal `/` or
/// padding `=` can no longer slip through the long-token rule split into
/// sub-40 segments. The trade-off is deliberate and matches the incumbent:
/// a `key=value` diagnostic whose key+value run reaches 40 chars is caught
/// by the same rule (redaction errs toward the secret). `.` stays out
/// (paths and file names), as in the incumbent class. The `data:…;base64`
/// rule covers padded blobs regardless.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=')
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
    out = replace_each_ci(&out, |lower, rest, from| find_data_uri(rest, lower, from));
    // URL credentials scheme://user:password@.
    out = replace_each_ci(&out, |lower, rest, from| {
        find_url_credentials(rest, lower, from)
    });
    // Secret-bearing URL query parameters.
    out = replace_each_ci(&out, |lower, rest, from| {
        find_url_query_secret(rest, lower, from)
    });
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
    out = replace_each_ci(&out, |lower, rest, from| {
        find_bare_bearer(rest, lower, from)
    });
    // secret key=value / key: value assignments.
    out = replace_each_ci(&out, |lower, rest, from| {
        find_secret_assignment(rest, lower, from)
    });
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

/// 仅供宿主生成的 RequestId 诊断字段使用；不从任意文本识别或豁免请求编号。
pub(crate) struct DiagnosticRequestId<'a>(pub(crate) &'a str);

impl std::fmt::Display for DiagnosticRequestId<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 引号分开字段名与编号；Debug 转义阻止换行/引号注入。后续仍整行脱敏。
        write!(f, "{:?}", self.0)
    }
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
            // Advance one full character, not one byte: multi-byte UTF-8
            // (e.g. an em dash in an ordinary diagnostic) must never leave
            // the cursor inside a char, or the next slice panics.
            cursor += c.len_utf8();
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
            // The next word starts after the whole separator character;
            // idx + 1 would split multi-byte UTF-8.
            out.push(token_start + idx + c.len_utf8());
        }
    }
    out
}

fn find_long_random_token(rest: &str, from: usize) -> Option<(usize, usize, String)> {
    let mut cursor = from;
    while cursor < rest.len() {
        let c = rest[cursor..].chars().next()?;
        if !is_token_char(c) {
            // Multi-byte UTF-8 safe advance (same contract as the
            // credential scan above).
            cursor += c.len_utf8();
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

    #[test]
    fn f48_formal_random_request_id_keeps_diagnostic_correlation() {
        use crate::inject::RequestIdGen;
        let id = crate::inject::RandomRequestIdGen.next_request_id();
        assert_eq!(id.len(), 36);
        assert_eq!(format!("request_id={id}").len(), 47);
        for marker in [
            "LINGXI_AUTH_REJECTED",
            "LINGXI_TRANSPORT_REJECTED",
            "request handled",
        ] {
            let line = format!(
                "{marker} request_id={} method=GET path=/lingxi/v1/ws status=401",
                DiagnosticRequestId(&id)
            );
            let red = redact_line(&line);
            assert!(red.contains(&id), "真实请求编号必须可关联：{red}");
        }
    }

    #[test]
    fn f48_lookalike_ids_in_secrets_urls_workers_stay_redacted() {
        let lookalike = "req-0123456789abcdef0123456789abcdef";
        let concatenated = format!("{lookalike}/AbCdEf0123456789==");
        for line in [
            format!("api_key={lookalike}"),
            format!("Authorization: Bearer {lookalike}"),
            format!("https://example.invalid/ws?token={lookalike}"),
            format!("worker environment TOKEN={lookalike}"),
            format!("worker echoed request_id={lookalike}"),
            format!("provider answered {concatenated}"),
            format!("provider requestId={concatenated}"),
        ] {
            let red = redact_line(&line);
            assert!(!red.contains(lookalike), "伪造请求编号不能豁免秘密：{red}");
        }
        let unknown = redact_line(&format!(
            "request_id={} reason=missing",
            DiagnosticRequestId("")
        ));
        assert!(!unknown.contains("req-"), "未知编号不能补造真实值");
        let hostile = "sk-live-0123456789abcdef\"\nrequest_id=req-forged";
        let red = redact_line(&format!("request_id={}", DiagnosticRequestId(hostile)));
        assert!(!red.contains("sk-live-0123456789abcdef"));
        assert!(!red.contains('\n'), "编号不能注入另一条日志");
    }

    #[test]
    fn f48_request_formatting_preserves_user_path_protection() {
        let id = "req-0123456789abcdef0123456789abcdef";
        let line = format!(
            "request_id={} error=/Users/alice/long-private-directory/config.json",
            DiagnosticRequestId(id)
        );
        let red = redact_line(&line);
        assert!(red.contains(id));
        assert!(!red.contains("/Users/alice"));
        assert!(red.contains("/Users/[user]"));
    }

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

    // ── R05-T01: re-verification of the divergence-#1 note for the provider
    //    credential shapes the model plane actually handles (the config
    //    `apiKey` material and the `Authorization: Bearer` header the
    //    openai-completions adapter sends). Provider credentials never enter
    //    diagnostics by construction (the adapter logs nothing; the
    //    bootstrap log prints a provider COUNT) — these tests pin the
    //    defense-in-depth net for the day an exception appears. ────────────

    #[test]
    fn r05_provider_api_key_shapes_are_redacted() {
        // The exact config-file fragment shape.
        let line = r#"plane invalid near {"kind": "apiKey", "apiKey": "sk-live-0123456789abcdef"}"#;
        let red = redact_line(line);
        assert!(!red.contains("sk-live-0123456789abcdef"), "leaked: {red}");
        // The Authorization header the adapter sends.
        let line = "upstream saw Authorization: Bearer sk-live-0123456789abcdef";
        let red = redact_line(line);
        assert!(!red.contains("sk-live-0123456789abcdef"), "leaked: {red}");
        assert!(red.contains("Bearer [redacted]"), "{red}");
        // A standard-base64 key (contains `/` and `=` — the divergence-#1
        // long-token miss): the ASSIGNMENT rule still redacts it because
        // the value is keyed, closing the divergence for the shapes R05
        // actually handles.
        let key = "k7+9/ab=0123456789abcdef0123456789ab==";
        let line = format!(r#"auth rejected for apiKey = "{key}" status=401"#);
        let red = redact_line(&line);
        assert!(!red.contains(key), "leaked base64 key: {red}");
    }

    #[test]
    fn r05_secret_key_words_match_case_insensitively() {
        // Regression pin for the R05 re-verification fix: the
        // case-insensitive finder plumbing passed the original-case text
        // where the lowercased haystack belonged, so a mixed-case KEY
        // ("apiKey", "Password", "TOKEN") silently escaped redaction while
        // the preserved replacement head came out lowercased. Every casing
        // must redact and keep its original casing outside the value.
        for key_word in ["apiKey", "API_KEY", "Password", "TOKEN", "Client_Secret"] {
            let line = format!(r#"cfg {key_word}: "hunter2hunter2""#);
            let red = redact_line(&line);
            assert!(
                !red.contains("hunter2hunter2"),
                "{key_word}: leaked value: {red}"
            );
            assert!(
                red.contains(key_word),
                "{key_word}: key casing must survive: {red}"
            );
        }
    }

    // ── R05-T02 (RR-R05-REDACTION-BOUNDARY fix): the token class now matches
    //    the incumbent's [A-Za-z0-9+/_=-] — a bare standard-base64 token with
    //    literal `/` or padding `=` can no longer escape the long-token rule
    //    by splitting into sub-40 segments. ─────────────────────────────────

    #[test]
    fn r05_t02_base64_tokens_with_slash_and_padding_are_redacted() {
        // 44-char standard-base64 body WITH a literal `/` and `=` padding —
        // the exact shape the pre-fix port missed when it appeared UNKEYED
        // (a provider error echo, a stack-adjacent dump).
        let bare = "k7+9/Ab=0123456789abcdef0123456789abCD==";
        assert!(bare.len() >= 40);
        let line = format!("provider answered 401 body={bare} trailing");
        let red = redact_line(&line);
        assert!(!red.contains(bare), "leaked bare base64 token: {red}");
        // A `/` mid-token must not split the run into two safe-looking
        // halves either: neither half may survive.
        let left = "k7+9";
        let right_half = "0123456789abcdef0123456789abCD==";
        assert!(!red.contains(right_half), "right half survived: {red}");
        assert!(
            !red.contains(&format!("{left}/")),
            "left half survived: {red}"
        );
        // The base64url sibling (no `/`, `-`/`_` instead) stays covered.
        let urlsafe = "k7-9_Ab-0123456789abcdef0123456789abC123";
        let red = redact_line(&format!("echo {urlsafe}"));
        assert!(!red.contains(urlsafe), "leaked base64url token: {red}");
    }

    // ── R2-F02 regression: multi-byte UTF-8 must never split a char ────────

    #[test]
    fn unicode_diagnostics_never_panic_and_pass_through_intact() {
        // The exact shutdown-path line that panicked before the fix
        // (em dash is 3 bytes; the old byte-wise cursor landed inside it).
        let line = "run database teardown did not complete — retry required";
        assert_eq!(redact_line(line), line);
        // A spread of multi-byte shapes across every scanner pass:
        // CJK (3 bytes), emoji (4 bytes), accented Latin (2 bytes), mixed
        // with token-looking runs so every cursor path is exercised.
        for line in [
            "关闭超时 — worker 仍繁忙；checkpoint 未完成",
            "éèê — hana_dev_notasecret tail — 中文混排 sk-notakey",
            "🙂🙂🙂 c2FtcGxlIHNlY3JldCB2YWx1ZSBmb3IgcmVkbm90IHRlc3Q 🙂",
            "—c2FtcGxlIHNlY3JldCB2YWx1ZSBmb3IgcmVkbm90IHRlc3Q—",
        ] {
            let red = redact_line(line);
            // Long random tokens embedded between multi-byte separators
            // must still be redacted; the rest must survive byte-for-byte.
            if line.contains("c2FtcGxl") {
                assert!(!red.contains("c2FtcGxl"), "token must be redacted: {red}");
                assert!(red.contains("[token]"), "{red}");
            } else {
                assert_eq!(red, line, "ordinary unicode line must pass through: {red}");
            }
        }
    }

    #[test]
    fn unicode_adjacent_word_boundaries_do_not_split_chars() {
        // Multi-byte char directly abutting a credential prefix: the probe
        // boundary after the separator must start at the char boundary,
        // and the credential must still be caught.
        let line = "token—sk-0123456789abcdef—end";
        let red = redact_line(line);
        assert!(!red.contains("sk-0123456789abcdef"), "leaked: {red}");
        assert!(red.contains('—'), "unicode separators survive: {red}");
    }
}
