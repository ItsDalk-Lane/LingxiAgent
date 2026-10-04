//! R05-T04: the service-side live-delta normalization chain — the Rust port
//! of the incumbent's three-layer streaming pipeline:
//!
//! 1. [`ReservedTagScanner`] ports `shared/reserved-tag-stream.ts`
//!    byte-faithfully (openTag / unknownStack / code are three separate
//!    states; escape + code protection; bounded pending-tag and stack-depth
//!    guards with conservative-passthrough fallback). All structural
//!    characters the scanner matches (`<`, `>`, `` ` ``, `~`, `\`, quotes,
//!    tag-name bytes) are ASCII, so the TS UTF-16 code-unit indexing ports
//!    to UTF-8 byte indexing one-to-one; non-ASCII content is only ever
//!    copied, and every slice index is an ASCII-delimiter position (a char
//!    boundary).
//! 2. [`ReservedTagLayer`] ports `core/events.ts`'s `ReservedTagParserBase`
//!    (block start/text/end routing + the just-ended leading-newline trim).
//!    The think layer runs FIRST (`think`/`thinking`/`mm:think`, orphan
//!    closers of the layer's own vocabulary pass through literally), the
//!    mood layer SECOND (`mood`/`pulse`/`reflect`, `dropUnknownOrphanClosers`
//!    — it is the chain tail in the incumbent).
//! 3. [`DeltaNormalizer`] maps the cleaned fragments onto the frozen wire
//!    vocabulary (contract §6 / S09): `model_call_delta` carries the
//!    per-phase fragment, `assistant_segment_*` the segment-structured view.
//!    D5 decisions:
//!    - text fragments are `final_answer` (the Rust adapters know the block
//!      kind at arrival; the incumbent's `unresolved` phase existed only for
//!      phase-at-end APIs whose textSignature resolved late — no such
//!      ambiguity exists on this chain, so `unresolved` never originates
//!      here);
//!    - reasoning fragments (provider-native `ModelTurnDelta::Reasoning`
//!      AND think-tag block text) are `reasoning`;
//!    - mood blocks are STRIPPED from the event stream: the frozen wire
//!      vocabulary has no mood event, and the canonical persisted message
//!      keeps the raw tagged text (the adapter accumulates verbatim), so
//!      history replays the same structure through
//!      [`split_reserved_tag_segments`] — live and history are same-source;
//!    - `commentary` has no source on this chain either (reserved
//!      vocabulary, documented in R05_INTERFACE_EVOLUTION).
//!
//!    Segment ids follow the incumbent shape with the run's turn number as
//!    the ordinal: `assistant:{turn}:reasoning:default` /
//!    `assistant:{turn}:text:default`. Both segments, once opened, stay
//!    open for the rest of the call (interleaved fragments append; the
//!    segment ends fire at `finish`) — one resident reasoning segment and
//!    one resident text segment per model call.

use lingxi_kernel::ports::ModelTurnDelta;
use lingxi_protocol::{AssistantPhase, SegmentKind};

/// The think layer's vocabulary (`core/events.ts` THINK_TAGS).
pub const THINK_TAGS: [&str; 3] = ["think", "thinking", "mm:think"];
/// The mood layer's vocabulary (`shared/internal-mood-block.ts`
/// INTERNAL_MOOD_TAGS).
pub const MOOD_TAGS: [&str; 3] = ["mood", "pulse", "reflect"];

/// Bounded-parse guard (F8/P6.4): a pending unterminated tag longer than
/// this is emitted literally instead of buffered without bound.
const MAX_PENDING_TAG_LEN: usize = 16 * 1024;
/// Unknown-tag open-stack depth cap: at the cap the segment switches to
/// conservative passthrough (no more orphan cleanup, never dropping body
/// text wholesale).
const MAX_TAG_STACK_DEPTH: usize = 128;

fn is_space_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

/// Length of the tag-name match at `start` (`[A-Za-z][A-Za-z0-9:_.-]*`),
/// 0 when there is none.
fn tag_name_len(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    if start >= bytes.len() || !bytes[start].is_ascii_alphabetic() {
        return 0;
    }
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'-') {
            i += 1;
        } else {
            break;
        }
    }
    i - start
}

/// Length of the longest proper prefix of `target` that is a suffix of
/// `buffer` (the cross-delta pending-tag detector).
fn trailing_prefix_len(buffer: &str, target: &str) -> usize {
    let max_check = buffer.len().min(target.len() - 1);
    for len in (1..=max_check).rev() {
        if buffer.ends_with(&target[..len]) {
            return len;
        }
    }
    0
}

/// The `^<\/([A-Za-z][A-Za-z0-9:_.-]*)>` shape (CLOSE_TAG_SHAPE): the full
/// literal length when `rest` starts with a well-formed close tag.
fn close_tag_shape_len(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') || bytes.get(1) != Some(&b'/') {
        return None;
    }
    let name_len = tag_name_len(rest, 2);
    if name_len == 0 {
        return None;
    }
    if bytes.get(2 + name_len) != Some(&b'>') {
        return None;
    }
    Some(2 + name_len + 1)
}

/// The `^<\/?[A-Za-z][A-Za-z0-9:_.-]*$` shape (PARTIAL_TAG_SHAPE): the WHOLE
/// rest is a not-yet-terminated tag opening.
fn partial_tag_shape_matches(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return false;
    }
    let name_start = if bytes.get(1) == Some(&b'/') { 2 } else { 1 };
    let name_len = tag_name_len(rest, name_start);
    name_len > 0 && name_start + name_len == rest.len()
}

/// One scanner output token (verbatim port of the TS union).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservedTagToken {
    Text(String),
    Open(String),
    Close(String),
}

struct TagMatch {
    literal: String,
    tag: String,
    is_open: bool,
}

enum TagAt {
    Match(TagMatch),
    Partial,
    NoMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeKind {
    Inline,
    Fence,
}

#[derive(Debug, Clone, Copy)]
struct CodeSpan {
    marker: u8,
    run: usize,
    kind: CodeKind,
}

/// The generic-text tag grammar match (F8/P6.2). `length` is always the
/// complete literal length.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GenericTagMatch {
    Open { name: String, length: usize },
    Close { name: String, length: usize },
    SelfClose { length: usize },
    Comment { length: usize },
    Cdata { length: usize },
    Decl { length: usize },
}

impl GenericTagMatch {
    fn length(&self) -> usize {
        match self {
            GenericTagMatch::Open { length, .. }
            | GenericTagMatch::Close { length, .. }
            | GenericTagMatch::SelfClose { length }
            | GenericTagMatch::Comment { length }
            | GenericTagMatch::Cdata { length }
            | GenericTagMatch::Decl { length } => *length,
        }
    }
}

enum GenericAt {
    Match(GenericTagMatch),
    Partial,
    NoMatch,
}

/// Ports `matchGenericTag`: quoted attribute values do not terminate on
/// `>`; comments / CDATA / declarations establish no open state; `partial`
/// near the buffer end means "cannot decide yet".
fn match_generic_tag(buf: &str, pos: usize) -> GenericAt {
    let bytes = buf.as_bytes();
    if bytes.get(pos) != Some(&b'<') {
        return GenericAt::NoMatch;
    }
    let rest = &buf[pos..];

    if let Some(after) = rest.strip_prefix("<!--") {
        return match after.find("-->") {
            None => GenericAt::Partial,
            Some(rel) => GenericAt::Match(GenericTagMatch::Comment {
                length: 4 + rel + 3,
            }),
        };
    }
    if let Some(after) = rest.strip_prefix("<![CDATA[") {
        return match after.find("]]>") {
            None => GenericAt::Partial,
            Some(rel) => GenericAt::Match(GenericTagMatch::Cdata {
                length: 9 + rel + 3,
            }),
        };
    }
    if rest.starts_with("<?") || rest.starts_with("<!") {
        return match rest.find('>') {
            None => GenericAt::Partial,
            Some(idx) => GenericAt::Match(GenericTagMatch::Decl { length: idx + 1 }),
        };
    }

    if rest.starts_with("</") {
        let name_len = tag_name_len(rest, 2);
        if name_len == 0 {
            return GenericAt::NoMatch;
        }
        return match rest.as_bytes().get(2 + name_len) {
            None => GenericAt::Partial,
            Some(b'>') => GenericAt::Match(GenericTagMatch::Close {
                name: rest[2..2 + name_len].to_string(),
                length: 2 + name_len + 1,
            }),
            Some(_) => GenericAt::NoMatch,
        };
    }

    let name_len = tag_name_len(rest, 1);
    if name_len == 0 {
        return GenericAt::NoMatch;
    }
    let name = rest[1..1 + name_len].to_string();
    let rbytes = rest.as_bytes();
    let mut i = 1 + name_len;
    let mut self_close = false;
    loop {
        if i >= rest.len() {
            return GenericAt::Partial;
        }
        let ch = rbytes[i];
        if ch == b'>' {
            break;
        }
        if ch == b'/' {
            if rbytes.get(i + 1) == Some(&b'>') {
                self_close = true;
                i += 2;
                break;
            }
            return GenericAt::NoMatch;
        }
        if is_space_byte(ch) {
            i += 1;
            continue;
        }
        // Attribute name: consume until whitespace/=/>/.
        let mut moved = false;
        while i < rest.len()
            && !is_space_byte(rbytes[i])
            && rbytes[i] != b'='
            && rbytes[i] != b'>'
            && rbytes[i] != b'/'
        {
            i += 1;
            moved = true;
        }
        if !moved {
            return GenericAt::NoMatch;
        }
        if i >= rest.len() {
            return GenericAt::Partial;
        }
        if rbytes[i] != b'=' {
            continue;
        }
        i += 1;
        if i >= rest.len() {
            return GenericAt::Partial;
        }
        let quote = rbytes[i];
        if quote == b'"' || quote == b'\'' {
            // A `>` inside a quoted value is not a terminator.
            match rest[i + 1..].find(quote as char) {
                None => return GenericAt::Partial,
                Some(rel) => {
                    i = i + 1 + rel + 1;
                    continue;
                }
            }
        }
        // Unquoted value: consume until whitespace or >.
        while i < rest.len() && !is_space_byte(rbytes[i]) && rbytes[i] != b'>' {
            i += 1;
        }
    }
    GenericAt::Match(if self_close {
        GenericTagMatch::SelfClose { length: i }
    } else {
        GenericTagMatch::Open { name, length: i }
    })
}

/// The streaming scanner for the internal reserved protocol tags (verbatim
/// port of `ReservedTagScanner`). Contract (F8/P6 three-concept split):
///
/// 1. Known internal protocol tag PAIRS are protocol, not text: wherever
///    they appear in one generation they structure it.
/// 2. Ordinary unknown markup keeps an open stack by the generic shape
///    rules: paired tags are preserved verbatim; only a close tag with NO
///    open record anywhere is protocol residue, cleaned only at the final
///    text boundary (`drop_unknown_orphan_closers`).
/// 3. Escape and code protection: `\<tag>` (any tag shape) and tags inside
///    inline/fenced code are literal text; the backslash is preserved into
///    the canonical source for the display layer's Markdown lexer.
pub struct ReservedTagScanner {
    buffer: String,
    open_tag: Option<String>,
    code: Option<CodeSpan>,
    unknown_stack: Vec<String>,
    conservative_passthrough: bool,
    literals: Vec<String>,
    drop_unknown_orphan_closers: bool,
}

impl ReservedTagScanner {
    pub fn new(tags: &[&str], drop_unknown_orphan_closers: bool) -> Self {
        let mut literals = Vec::with_capacity(tags.len() * 2);
        for tag in tags {
            literals.push(format!("<{tag}>"));
            literals.push(format!("</{tag}>"));
        }
        Self {
            buffer: String::new(),
            open_tag: None,
            code: None,
            unknown_stack: Vec::new(),
            conservative_passthrough: false,
            literals,
            drop_unknown_orphan_closers,
        }
    }

    /// Whether a protocol tag is currently open (the upper layer flushes an
    /// end event at segment boundaries).
    pub fn inside_tag(&self) -> Option<&str> {
        self.open_tag.as_deref()
    }

    pub fn feed(&mut self, delta: &str) -> Vec<ReservedTagToken> {
        self.buffer.push_str(delta);
        self.drain(false)
    }

    /// Flush the buffer: pending half tags / escapes / code markers resolve
    /// as literal text.
    pub fn flush(&mut self) -> Vec<ReservedTagToken> {
        self.drain(true)
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.open_tag = None;
        self.code = None;
        self.unknown_stack.clear();
        self.conservative_passthrough = false;
    }

    /// Matches a complete protocol tag literal at `pos`; `Partial` means the
    /// buffer tail could be a cross-delta half tag.
    fn match_tag_at(&self, buf: &str, pos: usize) -> TagAt {
        let rest = &buf[pos..];
        for literal in &self.literals {
            if rest.starts_with(literal.as_str()) {
                let is_open = !literal.starts_with("</");
                let tag = if is_open {
                    literal[1..literal.len() - 1].to_string()
                } else {
                    literal[2..literal.len() - 1].to_string()
                };
                return TagAt::Match(TagMatch {
                    literal: literal.clone(),
                    tag,
                    is_open,
                });
            }
        }
        if rest.starts_with('<') {
            for literal in &self.literals {
                if literal.len() > rest.len() && literal.starts_with(rest) {
                    return TagAt::Partial;
                }
            }
        }
        TagAt::NoMatch
    }

    fn drain(&mut self, is_flush: bool) -> Vec<ReservedTagToken> {
        let mut tokens = Vec::new();
        let mut text = String::new();
        let mut i = 0usize;
        // Take the buffer out so the state fields stay mutable while the
        // scan reads; the undrained tail is restored below (the TS original
        // reassigns `this.buffer = buf.slice(i)` on the same paths).
        let buf = std::mem::take(&mut self.buffer);
        macro_rules! push_text {
            () => {
                if !text.is_empty() {
                    tokens.push(ReservedTagToken::Text(std::mem::take(&mut text)));
                }
            };
        }

        while i < buf.len() {
            // ── protocol-block content mode: opaque; only the same-named
            //    close tag ends it ──
            if let Some(open_tag) = self.open_tag.clone() {
                let close_tag = format!("</{open_tag}>");
                if let Some(rel) = buf[i..].find(&close_tag) {
                    let idx = i + rel;
                    text.push_str(&buf[i..idx]);
                    push_text!();
                    tokens.push(ReservedTagToken::Close(open_tag));
                    self.open_tag = None;
                    i = idx + close_tag.len();
                    continue;
                }
                if !is_flush {
                    let hold_len = trailing_prefix_len(&buf[i..], &close_tag);
                    let safe_end = buf.len() - hold_len;
                    text.push_str(&buf[i..safe_end]);
                    push_text!();
                    self.buffer = buf[safe_end..].to_string();
                    return tokens;
                }
                text.push_str(&buf[i..]);
                push_text!();
                self.buffer = String::new();
                return tokens;
            }

            let b = buf.as_bytes()[i];

            // ── code protection mode: only the matching close marker ends it ──
            if let Some(code) = self.code {
                if b == code.marker {
                    let mut run = 1;
                    while buf.as_bytes().get(i + run) == Some(&b) {
                        run += 1;
                    }
                    if i + run >= buf.len() && !is_flush {
                        break; // the tail marker run may still grow — pend
                    }
                    text.push_str(&buf[i..i + run]);
                    i += run;
                    let closes = match code.kind {
                        CodeKind::Fence => run >= code.run,
                        CodeKind::Inline => run == code.run,
                    };
                    if closes {
                        self.code = None;
                    }
                    continue;
                }
                let ch_len = buf[i..].chars().next().map(char::len_utf8).unwrap_or(1);
                text.push_str(&buf[i..i + ch_len]);
                i += ch_len;
                continue;
            }

            // ── escape: `\<any tag shape>` is a protected literal (the
            //    backslash is preserved for the display layer) ──
            if b == b'\\' {
                if i + 1 >= buf.len() && !is_flush {
                    break; // may be escaping a tag in the next chunk — pend
                }
                if buf.as_bytes().get(i + 1) == Some(&b'<') {
                    match match_generic_tag(&buf, i + 1) {
                        GenericAt::Partial if !is_flush => break,
                        GenericAt::Match(generic) => {
                            text.push_str(&buf[i..i + 1 + generic.length()]);
                            i += 1 + generic.length();
                            continue;
                        }
                        _ => {}
                    }
                }
                text.push_str(&buf[i..i + 1]);
                i += 1;
                continue;
            }

            // ── code markers: >=3 backticks/tildes fence; 1~2 backticks inline ──
            if b == b'`' || b == b'~' {
                let mut run = 1;
                while buf.as_bytes().get(i + run) == Some(&b) {
                    run += 1;
                }
                if i + run >= buf.len() && !is_flush {
                    break; // the tail marker run may still grow — pend
                }
                text.push_str(&buf[i..i + run]);
                i += run;
                if run >= 3 {
                    self.code = Some(CodeSpan {
                        marker: b,
                        run,
                        kind: CodeKind::Fence,
                    });
                } else if b == b'`' {
                    self.code = Some(CodeSpan {
                        marker: b'`',
                        run,
                        kind: CodeKind::Inline,
                    });
                }
                continue;
            }

            // ── tags ──
            if b == b'<' {
                match self.match_tag_at(&buf, i) {
                    TagAt::Partial if !is_flush => break,
                    TagAt::Match(matched) => {
                        if matched.is_open {
                            push_text!();
                            self.open_tag = Some(matched.tag.clone());
                            tokens.push(ReservedTagToken::Open(matched.tag));
                        } else {
                            // In-vocabulary orphan closer: literal passthrough
                            // (the escape/teaching contract); residue cleanup
                            // is the chain tail's out-of-vocabulary rule.
                            text.push_str(&matched.literal);
                        }
                        i += matched.literal.len();
                        continue;
                    }
                    _ => {}
                }
                match match_generic_tag(&buf, i) {
                    GenericAt::Partial if !is_flush => {
                        // Bounded guard: an over-long pending tag region goes
                        // out literally instead of buffering without bound.
                        if buf.len() - i <= MAX_PENDING_TAG_LEN {
                            break;
                        }
                        text.push('<');
                        i += 1;
                        continue;
                    }
                    GenericAt::Match(generic) => {
                        let generic_length = generic.length();
                        let literal = &buf[i..i + generic_length];
                        match &generic {
                            GenericTagMatch::Close { name, .. } => {
                                if let Some(pos) =
                                    self.unknown_stack.iter().rposition(|n| n == name)
                                {
                                    // Paired unknown markup: preserved, the
                                    // open record pops.
                                    self.unknown_stack.remove(pos);
                                    text.push_str(literal);
                                } else if self.drop_unknown_orphan_closers
                                    && !self.conservative_passthrough
                                {
                                    // Genuine orphan close with no open
                                    // record: protocol residue, cleaned at
                                    // this final boundary only.
                                } else {
                                    text.push_str(literal);
                                }
                            }
                            GenericTagMatch::Open { name, .. } => {
                                text.push_str(literal);
                                if !self.conservative_passthrough {
                                    if self.unknown_stack.len() >= MAX_TAG_STACK_DEPTH {
                                        // Depth cap: conservative passthrough
                                        // for the rest of the segment (never
                                        // dropping body text wholesale).
                                        self.conservative_passthrough = true;
                                    } else {
                                        self.unknown_stack.push(name.clone());
                                    }
                                }
                            }
                            // self-close / comment / CDATA / declaration:
                            // preserved verbatim, no open state.
                            _ => text.push_str(literal),
                        }
                        i += generic_length;
                        continue;
                    }
                    _ => {}
                }
                if self.drop_unknown_orphan_closers
                    && !self.conservative_passthrough
                    && buf[i..].starts_with("</")
                {
                    let rest = &buf[i..];
                    if let Some(close_len) = close_tag_shape_len(rest) {
                        // Out-of-vocabulary residue (defensive: the generic
                        // shape above already covers this; conservative floor).
                        i += close_len;
                        continue;
                    }
                    if !is_flush && partial_tag_shape_matches(rest) {
                        break; // the tail may be a cross-delta close tag — pend
                    }
                }
                text.push('<');
                i += 1;
                continue;
            }

            let ch_len = buf[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            text.push_str(&buf[i..i + ch_len]);
            i += ch_len;
        }

        push_text!();
        self.buffer = buf[i..].to_string();
        tokens
    }
}

/// The parser-layer event of one reserved-tag family (the port of
/// `core/events.ts` `ReservedTagParserBase`'s emitted events, names
/// generalized: the think layer and the mood layer share this shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagLayerEvent {
    /// Entered a vocabulary block (the layer's `*_start`).
    BlockStart,
    /// Text INSIDE the block (the layer's `*_text`).
    BlockText(String),
    /// The block closed (the layer's `*_end`).
    BlockEnd,
    /// Text outside any block of this layer's vocabulary.
    Text(String),
}

/// One reserved-tag parser layer (the port of `ReservedTagParserBase`):
/// wraps the scanner, tracks in-block state and applies the just-ended
/// leading-newline trim (layout newlines between a block and the body text
/// never enter the body).
pub struct ReservedTagLayer {
    scanner: ReservedTagScanner,
    in_tag: bool,
    just_ended: bool,
}

impl ReservedTagLayer {
    pub fn new(tags: &[&str], drop_unknown_orphan_closers: bool) -> Self {
        Self {
            scanner: ReservedTagScanner::new(tags, drop_unknown_orphan_closers),
            in_tag: false,
            just_ended: false,
        }
    }

    pub fn feed(&mut self, delta: &str) -> Vec<TagLayerEvent> {
        let tokens = self.scanner.feed(delta);
        tokens
            .into_iter()
            .filter_map(|token| self.handle_token(token))
            .collect()
    }

    /// Flush: pending half tags resolve as literal text; an unclosed block
    /// emits its end event (the incumbent's flush contract).
    pub fn flush(&mut self) -> Vec<TagLayerEvent> {
        let tokens = self.scanner.flush();
        let mut out: Vec<TagLayerEvent> = tokens
            .into_iter()
            .filter_map(|token| self.handle_token(token))
            .collect();
        if self.in_tag {
            out.push(TagLayerEvent::BlockEnd);
            self.in_tag = false;
            self.just_ended = true;
        }
        out
    }

    pub fn reset(&mut self) {
        self.scanner.reset();
        self.in_tag = false;
        self.just_ended = false;
    }

    fn handle_token(&mut self, token: ReservedTagToken) -> Option<TagLayerEvent> {
        match token {
            ReservedTagToken::Open(_) => {
                self.in_tag = true;
                Some(TagLayerEvent::BlockStart)
            }
            ReservedTagToken::Close(_) => {
                self.in_tag = false;
                self.just_ended = true;
                Some(TagLayerEvent::BlockEnd)
            }
            ReservedTagToken::Text(raw) => {
                let text = if !self.in_tag && self.just_ended {
                    self.just_ended = false;
                    raw.trim_start_matches('\n').to_string()
                } else {
                    raw
                };
                if text.is_empty() {
                    return None;
                }
                Some(if self.in_tag {
                    TagLayerEvent::BlockText(text)
                } else {
                    TagLayerEvent::Text(text)
                })
            }
        }
    }
}

/// One normalized output of the live-delta chain, ready to be wrapped into
/// the run's key events by the driver (R05-T04 D6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormEvent {
    /// `model_call_delta`: the per-phase fragment (raw-phase delta).
    ModelDelta {
        phase: AssistantPhase,
        delta: String,
    },
    /// `assistant_segment_start`.
    SegmentStart {
        segment_id: String,
        kind: SegmentKind,
        phase: AssistantPhase,
    },
    /// `assistant_segment_delta`.
    SegmentDelta {
        segment_id: String,
        delta: String,
        phase: AssistantPhase,
    },
    /// `assistant_segment_end`.
    SegmentEnd {
        segment_id: String,
        phase: AssistantPhase,
    },
}

/// The per-model-call normalizer (R05-T04): maps the provider's
/// [`ModelTurnDelta`] stream through the think/mood reserved-tag chain onto
/// the frozen wire vocabulary. One instance per model call; `finish` at the
/// call's terminal flushes the tag parsers and closes the open segments.
pub struct DeltaNormalizer {
    segment_base: String,
    think: ReservedTagLayer,
    mood: ReservedTagLayer,
    reasoning_open: bool,
    text_open: bool,
}

impl DeltaNormalizer {
    /// `turn` is the run's 1-based model-turn index (the incumbent's
    /// message-ordinal slot); one model call = one assistant message.
    pub fn new(turn: u32) -> Self {
        Self {
            segment_base: format!("assistant:{turn}"),
            think: ReservedTagLayer::new(&THINK_TAGS, false),
            // The mood layer is the chain tail in the incumbent: it owns the
            // out-of-vocabulary orphan-closer cleanup.
            mood: ReservedTagLayer::new(&MOOD_TAGS, true),
            reasoning_open: false,
            text_open: false,
        }
    }

    /// Feeds one provider delta; returns the normalized events in order.
    pub fn feed(&mut self, delta: &ModelTurnDelta) -> Vec<NormEvent> {
        let mut out = Vec::new();
        match delta {
            // Provider-native reasoning bypasses the tag chain (the
            // incumbent's thinkingDeltaFromEvent path).
            ModelTurnDelta::Reasoning(text) => self.emit_reasoning(text, &mut out),
            ModelTurnDelta::Text(raw) => {
                for event in self.think.feed(raw) {
                    self.route_think(event, &mut out);
                }
            }
        }
        out
    }

    /// The call's terminal: flush both parser layers (pending half tags
    /// resolve as literal text into the CURRENT segment), then close the
    /// open segments (reasoning first — the incumbent's
    /// `finishOpenSegments` order).
    pub fn finish(&mut self) -> Vec<NormEvent> {
        let mut out = Vec::new();
        for event in self.think.flush() {
            self.route_think(event, &mut out);
        }
        for event in self.mood.flush() {
            self.route_mood(event, &mut out);
        }
        if self.reasoning_open {
            self.reasoning_open = false;
            out.push(NormEvent::SegmentEnd {
                segment_id: self.reasoning_segment_id(),
                phase: AssistantPhase::Reasoning,
            });
        }
        if self.text_open {
            self.text_open = false;
            out.push(NormEvent::SegmentEnd {
                segment_id: self.text_segment_id(),
                phase: AssistantPhase::FinalAnswer,
            });
        }
        out
    }

    fn reasoning_segment_id(&self) -> String {
        format!("{}:reasoning:default", self.segment_base)
    }

    fn text_segment_id(&self) -> String {
        format!("{}:text:default", self.segment_base)
    }

    fn route_think(&mut self, event: TagLayerEvent, out: &mut Vec<NormEvent>) {
        match event {
            // Block boundaries are chain-internal (the frozen vocabulary has
            // no thinking_start/thinking_end events; the reasoning segment
            // opens lazily on the first fragment).
            TagLayerEvent::BlockStart | TagLayerEvent::BlockEnd => {}
            TagLayerEvent::BlockText(text) => self.emit_reasoning(&text, out),
            TagLayerEvent::Text(text) => {
                for event in self.mood.feed(&text) {
                    self.route_mood(event, out);
                }
            }
        }
    }

    fn route_mood(&mut self, event: TagLayerEvent, out: &mut Vec<NormEvent>) {
        match event {
            // D5: mood blocks are structured OUT of the event stream (no
            // mood events in the frozen vocabulary); the canonical persisted
            // message keeps the raw tagged text, so history replays the same
            // structure through split_reserved_tag_segments.
            TagLayerEvent::BlockStart | TagLayerEvent::BlockEnd | TagLayerEvent::BlockText(_) => {}
            TagLayerEvent::Text(text) => self.emit_text(&text, out),
        }
    }

    fn emit_reasoning(&mut self, text: &str, out: &mut Vec<NormEvent>) {
        if text.is_empty() {
            return;
        }
        let segment_id = self.reasoning_segment_id();
        if !self.reasoning_open {
            self.reasoning_open = true;
            out.push(NormEvent::SegmentStart {
                segment_id: segment_id.clone(),
                kind: SegmentKind::Reasoning,
                phase: AssistantPhase::Reasoning,
            });
        }
        out.push(NormEvent::ModelDelta {
            phase: AssistantPhase::Reasoning,
            delta: text.to_string(),
        });
        out.push(NormEvent::SegmentDelta {
            segment_id,
            delta: text.to_string(),
            phase: AssistantPhase::Reasoning,
        });
    }

    fn emit_text(&mut self, text: &str, out: &mut Vec<NormEvent>) {
        if text.is_empty() {
            return;
        }
        let segment_id = self.text_segment_id();
        if !self.text_open {
            self.text_open = true;
            out.push(NormEvent::SegmentStart {
                segment_id: segment_id.clone(),
                kind: SegmentKind::Text,
                phase: AssistantPhase::FinalAnswer,
            });
        }
        out.push(NormEvent::ModelDelta {
            phase: AssistantPhase::FinalAnswer,
            delta: text.to_string(),
        });
        out.push(NormEvent::SegmentDelta {
            segment_id,
            delta: text.to_string(),
            phase: AssistantPhase::FinalAnswer,
        });
    }
}

/// One segment of the one-shot split (the port of `ReservedTagSegment`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservedTagSegment {
    Text(String),
    Block { tag: String, content: String },
}

/// The one-shot (non-streaming) split of a COMPLETE text into alternating
/// text / tag-block segments (the port of `splitReservedTagSegments`) — the
/// history re-render entry. The caller is the final text boundary, so the
/// orphan-closer shape rule is on. The live chain and this split share the
/// same scanner: live segment deltas and history re-renders are same-source.
pub fn split_reserved_tag_segments(content: &str, tags: &[&str]) -> Vec<ReservedTagSegment> {
    let mut scanner = ReservedTagScanner::new(tags, true);
    let mut tokens = scanner.feed(content);
    tokens.extend(scanner.flush());
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut block: Option<(String, String)> = None;
    macro_rules! push_text {
        () => {
            if !text.is_empty() {
                segments.push(ReservedTagSegment::Text(std::mem::take(&mut text)));
            }
        };
    }
    for token in tokens {
        match token {
            ReservedTagToken::Open(tag) => {
                push_text!();
                block = Some((tag, String::new()));
            }
            ReservedTagToken::Close(_) => {
                if let Some((tag, content)) = block.take() {
                    segments.push(ReservedTagSegment::Block { tag, content });
                }
            }
            ReservedTagToken::Text(t) => {
                if let Some((_, content)) = &mut block {
                    content.push_str(&t);
                } else {
                    text.push_str(&t);
                }
            }
        }
    }
    // An unclosed tail: the scanner's flush already emitted the content as
    // text; keep it as text here (the incumbent reconstructs the open tag).
    if let Some((tag, content)) = block.take() {
        text.push_str(&format!("<{tag}>{content}"));
    }
    push_text!();
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(tokens: &[ReservedTagToken]) -> String {
        tokens
            .iter()
            .filter_map(|t| match t {
                ReservedTagToken::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn known_tags_structure_wherever_they_appear() {
        let mut scanner = ReservedTagScanner::new(&THINK_TAGS, false);
        // A complete pair inside ONE feed structures in that same feed
        // (in-block content emits as soon as the close tag is present).
        let tokens = scanner.feed("before <think>inner</think> after");
        assert_eq!(
            tokens,
            vec![
                ReservedTagToken::Text("before ".to_string()),
                ReservedTagToken::Open("think".to_string()),
                ReservedTagToken::Text("inner".to_string()),
                ReservedTagToken::Close("think".to_string()),
                ReservedTagToken::Text(" after".to_string()),
            ]
        );
        assert!(scanner.flush().is_empty());
    }

    #[test]
    fn half_tags_pend_across_feeds() {
        let mut scanner = ReservedTagScanner::new(&THINK_TAGS, false);
        // The safe prefix emits; the half tag pends.
        assert_eq!(
            scanner.feed("abc<thi"),
            vec![ReservedTagToken::Text("abc".to_string())]
        );
        let tokens = scanner.feed("nk>xyz");
        assert_eq!(
            tokens,
            vec![
                ReservedTagToken::Open("think".to_string()),
                ReservedTagToken::Text("xyz".to_string()),
            ]
        );
        // In-block content emits minus a possible close-tag prefix.
        assert_eq!(
            scanner.feed("1</th"),
            vec![ReservedTagToken::Text("1".to_string())]
        );
        let tokens = scanner.feed("ink>2");
        assert_eq!(
            tokens,
            vec![
                ReservedTagToken::Close("think".to_string()),
                ReservedTagToken::Text("2".to_string()),
            ]
        );
    }

    #[test]
    fn escapes_and_code_spans_are_literal() {
        let mut scanner = ReservedTagScanner::new(&THINK_TAGS, false);
        let tokens = scanner.feed("\\<think> not a tag");
        assert_eq!(texts(&tokens), "\\<think> not a tag");
        let tokens = scanner.feed(" `<think>` ");
        assert_eq!(texts(&tokens), " `<think>` ");
        // The fence CLOSER at the buffer tail pends (the run may grow).
        let tokens = scanner.feed("```\n<think>\n```");
        assert_eq!(texts(&tokens), "```\n<think>\n");
        let tokens = scanner.flush();
        assert_eq!(texts(&tokens), "```");
        assert!(scanner.inside_tag().is_none());
    }

    #[test]
    fn in_vocabulary_orphan_closers_pass_literally() {
        let mut scanner = ReservedTagScanner::new(&THINK_TAGS, false);
        let tokens = scanner.feed("a </think> b");
        assert_eq!(texts(&tokens), "a </think> b");
    }

    #[test]
    fn out_of_vocabulary_orphan_closers_drop_only_at_the_final_boundary() {
        // Middle layer (no drop): preserved.
        let mut middle = ReservedTagScanner::new(&THINK_TAGS, false);
        assert_eq!(texts(&middle.feed("a </custom> b")), "a </custom> b");
        // Chain tail (drop): cleaned.
        let mut tail = ReservedTagScanner::new(&MOOD_TAGS, true);
        assert_eq!(texts(&tail.feed("a </custom> b")), "a  b");
        // But a PAIRED unknown tag is preserved even at the tail.
        let mut tail = ReservedTagScanner::new(&MOOD_TAGS, true);
        assert_eq!(
            texts(&tail.feed("a <custom>x</custom> b")),
            "a <custom>x</custom> b"
        );
    }

    #[test]
    fn quoted_attributes_do_not_end_tags_and_unknown_markup_is_preserved() {
        let mut scanner = ReservedTagScanner::new(&MOOD_TAGS, true);
        let tokens = scanner.feed("<x:item a=\"1>0\">v</x:item>");
        assert_eq!(texts(&tokens), "<x:item a=\"1>0\">v</x:item>");
    }

    #[test]
    fn comments_cdata_and_declarations_pass_verbatim() {
        let mut scanner = ReservedTagScanner::new(&MOOD_TAGS, true);
        let tokens = scanner.feed("a <!-- c --> b <![CDATA[x]]> <?pi?> <!doctype html>");
        assert_eq!(
            texts(&tokens),
            "a <!-- c --> b <![CDATA[x]]> <?pi?> <!doctype html>"
        );
    }

    #[test]
    fn the_pending_tag_bound_falls_back_to_literal() {
        let mut scanner = ReservedTagScanner::new(&MOOD_TAGS, true);
        // The safe prefix emits on the first feed; the half tag pends.
        assert_eq!(texts(&scanner.feed("pre <unfinished")), "pre ");
        // Grow the pending region past the cap with non-tag bytes: the
        // pending tag switches to literal passthrough.
        let long = format!("{:->width$}", "", width = MAX_PENDING_TAG_LEN + 8);
        let tokens = scanner.feed(&long);
        assert!(
            texts(&tokens).starts_with("<unfinished"),
            "the over-long pending region emits literally: {}",
            &texts(&tokens)[..40.min(texts(&tokens).len())]
        );
    }

    #[test]
    fn the_unknown_stack_depth_cap_switches_to_conservative_passthrough() {
        let mut scanner = ReservedTagScanner::new(&MOOD_TAGS, true);
        let mut payload = String::new();
        for n in 0..MAX_TAG_STACK_DEPTH {
            payload.push_str(&format!("<u{n}>"));
        }
        scanner.feed(&payload);
        // At the cap the NEXT unknown open switches the mode; from then on
        // even an orphan closer passes through (no more cleanup).
        let tokens = scanner.feed("<overflow>");
        assert!(texts(&tokens).contains("<overflow>"));
        let tokens = scanner.feed("</never-opened>");
        assert_eq!(texts(&tokens), "</never-opened>");
    }

    #[test]
    fn layer_trims_the_layout_newline_after_a_block() {
        let mut layer = ReservedTagLayer::new(&THINK_TAGS, false);
        let events = layer.feed("<think>abc</think>\n\nbody");
        assert_eq!(
            events,
            vec![
                TagLayerEvent::BlockStart,
                TagLayerEvent::BlockText("abc".to_string()),
                TagLayerEvent::BlockEnd,
                TagLayerEvent::Text("body".to_string()),
            ]
        );
    }

    #[test]
    fn layer_flush_closes_an_unclosed_block() {
        let mut layer = ReservedTagLayer::new(&THINK_TAGS, false);
        // In-block text emits on the feed itself (minus any close-tag
        // prefix pend); the flush only closes the block.
        let events = layer.feed("<think>tail");
        assert_eq!(
            events,
            vec![
                TagLayerEvent::BlockStart,
                TagLayerEvent::BlockText("tail".to_string()),
            ]
        );
        let events = layer.flush();
        assert_eq!(events, vec![TagLayerEvent::BlockEnd]);
    }

    #[test]
    fn normalizer_separates_think_mood_and_body() {
        let mut norm = DeltaNormalizer::new(1);
        let mut out = norm.feed(&ModelTurnDelta::Text(
            "<think>why</think>\n<mood>glad</mood>hello".to_string(),
        ));
        out.extend(norm.finish());
        // Reasoning fragment → reasoning segment; mood stripped entirely;
        // body → final_answer text segment; both segments close at finish.
        let kinds: Vec<&str> = out
            .iter()
            .map(|e| match e {
                NormEvent::SegmentStart { kind, .. } => match kind {
                    SegmentKind::Reasoning => "start:reasoning",
                    SegmentKind::Text => "start:text",
                },
                NormEvent::SegmentDelta { phase, .. } | NormEvent::ModelDelta { phase, .. } => {
                    match phase {
                        AssistantPhase::Reasoning => "delta:reasoning",
                        AssistantPhase::FinalAnswer => "delta:final",
                        _ => "delta:other",
                    }
                }
                NormEvent::SegmentEnd { phase, .. } => match phase {
                    AssistantPhase::Reasoning => "end:reasoning",
                    _ => "end:final",
                },
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "start:reasoning",
                "delta:reasoning",
                "delta:reasoning",
                "start:text",
                "delta:final",
                "delta:final",
                "end:reasoning",
                "end:final",
            ]
        );
        let body: String = out
            .iter()
            .filter_map(|e| match e {
                NormEvent::SegmentDelta {
                    phase: AssistantPhase::FinalAnswer,
                    delta,
                    ..
                } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(body, "hello", "mood content never enters the event stream");
    }

    #[test]
    fn normalizer_routes_native_reasoning_without_scanning() {
        let mut norm = DeltaNormalizer::new(2);
        let out = norm.feed(&ModelTurnDelta::Reasoning("<mood>raw</mood>".to_string()));
        // Native reasoning is NOT tag-scanned: the fragment is verbatim.
        let delta: String = out
            .iter()
            .filter_map(|e| match e {
                NormEvent::ModelDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(delta, "<mood>raw</mood>");
    }

    #[test]
    fn split_segments_replays_the_stream_structure() {
        let source = "pre <think>why</think> mid <mood>glad</mood> post";
        let segments = split_reserved_tag_segments(
            source,
            &["think", "thinking", "mm:think", "mood", "pulse", "reflect"],
        );
        assert_eq!(
            segments,
            vec![
                ReservedTagSegment::Text("pre ".to_string()),
                ReservedTagSegment::Block {
                    tag: "think".to_string(),
                    content: "why".to_string()
                },
                ReservedTagSegment::Text(" mid ".to_string()),
                ReservedTagSegment::Block {
                    tag: "mood".to_string(),
                    content: "glad".to_string()
                },
                ReservedTagSegment::Text(" post".to_string()),
            ]
        );
    }
}
