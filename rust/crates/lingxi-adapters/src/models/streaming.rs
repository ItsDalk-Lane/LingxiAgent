//! A bounded SSE (`text/event-stream`) frame decoder (R05-T03): the shared
//! inbound half of every streaming protocol family (openai-completions
//! streaming mode, anthropic-messages SSE, google `:streamGenerateContent`,
//! the codex responses stream).
//!
//! Contract stance:
//! - Frames are delivered ONLY on their blank-line terminator (the WHATWG
//!   dispatch rule) — a partial event at end-of-stream is DISCARDED and
//!   reported, never half-parsed into a turn.
//! - `data:` lines of one event join with `\n`; the `event:` field is
//!   carried through verbatim (anthropic's event vocabulary needs it);
//!   `id:`/`retry:` fields are ignored deliberately (the adapter never
//!   reconnects a stream — a truncated stream is a loud protocol error of
//!   the family layer, not a silent resume).
//! - Decoding is strict UTF-8 per line (ASCII terminators can never split
//!   a multi-byte sequence, so line-granularity strictness is whole-stream
//!   strictness) and the undelivered buffer is bounded — a runaway or
//!   hostile stream hits [`ErrorCode::InvalidMessage`], never unbounded
//!   memory.

use lingxi_kernel::ports::{ModelTurnDelta, TurnDeltaSink, TurnDeltaSinkClosed};
use lingxi_protocol::{ErrorCode, ProtocolError};

/// The explicit no-live-consumer sink (R05-T04): accepts every delta and
/// delivers nothing. Used where a terminal-only consumer drives a streaming
/// decode (offline/golden parse entries, tests); production runs the
/// driver's real bounded-channel sink.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullDeltaSink;

impl TurnDeltaSink for NullDeltaSink {
    fn emit<'a>(
        &'a self,
        _delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

/// One complete SSE event: the joined `data` payload plus the optional
/// `event` field (None when the frame carried none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Hard bound on undelivered bytes (a frame's data has no protocol reason
/// to exceed this; the bound is loud, never a silent truncation). This ALSO
/// serves as the whole-stream total-read bound of the streaming read loop
/// ([`super::dispatch::drive_sse_stream`]) — deliberately TIGHTER than the
/// R05_BASELINE pre-registered `stream_total_buffer_max_bytes` of 16 MiB
/// (§8: a tighter bound than the pre-registration needs no new approval;
/// the pre-registered value is the ceiling, not the target).
pub const SSE_BUFFER_LIMIT: usize = 8 * 1024 * 1024;

/// Hard bound on ONE frame's joined `data` payload (the R05_BASELINE
/// pre-registered `sse_single_frame_max_bytes`, §8). A single SSE event has
/// no protocol reason to approach this — a runaway frame is refused loudly
/// (and the stream read is cancelled), never truncated mid-frame.
pub const SSE_FRAME_MAX_BYTES: usize = 1024 * 1024;

/// Hard bound on ONE tool call's accumulated argument payload while a
/// stream is open (the R05_BASELINE pre-registered
/// `tool_arguments_max_bytes`, §8). Enforced by the per-family accumulators
/// on the fragments they concatenate; families whose arguments arrive
/// complete inside one frame are covered transitively by
/// [`SSE_FRAME_MAX_BYTES`]. Loud, never a silent truncation.
pub const TOOL_ARGUMENTS_MAX_BYTES: usize = 1024 * 1024;

/// The incremental decoder. Feed arbitrary byte chunks; complete events
/// come back in order.
pub struct SseDecoder {
    buf: Vec<u8>,
    event_field: Option<String>,
    data_lines: Vec<String>,
    /// Joined length of `data_lines` (newlines included) — the per-frame
    /// bound ([`SSE_FRAME_MAX_BYTES`]) is enforced against it.
    data_bytes: usize,
    bom_checked: bool,
}

/// The outcome of [`SseDecoder::finish`]: any events completed by the
/// trailing bytes, plus whether a PARTIAL event (data/event lines never
/// terminated by a blank line) was discarded at the end of stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFinish {
    pub events: Vec<SseEvent>,
    pub discarded_partial: bool,
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Decodes a COMPLETE SSE body into its events (the offline/golden half of
/// the streaming contract: one shot, strict frame parsing, an unterminated
/// trailing frame is loud). Production reads use [`SseDecoder`] directly
/// through `dispatch::drive_sse_stream` (incremental).
pub fn decode_complete_body(body: &str) -> Result<Vec<SseEvent>, ProtocolError> {
    let mut decoder = SseDecoder::new();
    let mut events = decoder.feed(body.as_bytes())?;
    let finish = decoder.finish()?;
    events.extend(finish.events);
    if finish.discarded_partial {
        return Err(ProtocolError::new(
            ErrorCode::InvalidMessage,
            "SSE body ended with an unterminated trailing frame; the partial event is \
             discarded and reported, never half-parsed"
                .to_string(),
            false,
        ));
    }
    Ok(events)
}

impl SseDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            event_field: None,
            data_lines: Vec::new(),
            data_bytes: 0,
            bom_checked: false,
        }
    }

    /// Feeds one chunk; returns the events it completed (in order).
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ProtocolError> {
        if self.buf.len() + chunk.len() > SSE_BUFFER_LIMIT {
            return Err(ProtocolError::new(
                ErrorCode::InvalidMessage,
                format!(
                    "SSE stream exceeded the {}-byte undelivered buffer bound; refusing \
                     (a runaway stream is a protocol failure, never an unbounded buffer)",
                    SSE_BUFFER_LIMIT
                ),
                false,
            ));
        }
        self.buf.extend_from_slice(chunk);
        self.drain_lines()
    }

    /// End of stream: the remaining bytes form one final line (the spec's
    /// EOF rule), after which any pending UNTERMINATED event is discarded
    /// and reported — the family layer decides whether a clean terminal
    /// marker was seen (a missing one is the family's loud error).
    pub fn finish(&mut self) -> Result<SseFinish, ProtocolError> {
        let mut events = Vec::new();
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            let line = std::str::from_utf8(&line).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    "SSE stream is not valid UTF-8".to_string(),
                    false,
                )
            })?;
            if let Some(event) = self.process_line(line.trim_end_matches('\r'))? {
                events.push(event);
            }
        }
        let discarded_partial = !self.data_lines.is_empty() || self.event_field.is_some();
        self.data_lines.clear();
        self.data_bytes = 0;
        self.event_field = None;
        Ok(SseFinish {
            events,
            discarded_partial,
        })
    }

    /// Extracts and processes every complete line in the buffer. A trailing
    /// `\r` with no following byte stays buffered (it may pair with the
    /// next chunk's `\n`).
    fn drain_lines(&mut self) -> Result<Vec<SseEvent>, ProtocolError> {
        let mut events = Vec::new();
        loop {
            if !self.bom_checked {
                self.bom_checked = true;
                if self.buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    self.buf.drain(..3);
                } else if self.buf.len() < 3
                    && !self.buf.is_empty()
                    && [0xEF, 0xBB].contains(&self.buf[0])
                {
                    // Possible split BOM — wait for more bytes.
                    break;
                }
            }
            let mut terminator: Option<(usize, usize)> = None; // (pos, len)
            for (index, byte) in self.buf.iter().enumerate() {
                match byte {
                    b'\n' => {
                        terminator = Some((index, 1));
                        break;
                    }
                    b'\r' => {
                        if index + 1 == self.buf.len() {
                            // Trailing CR: may pair with a LF in the next
                            // chunk — keep it buffered.
                            break;
                        }
                        terminator =
                            Some((index, if self.buf[index + 1] == b'\n' { 2 } else { 1 }));
                        break;
                    }
                    _ => {}
                }
            }
            let Some((pos, len)) = terminator else { break };
            let line_bytes: Vec<u8> = self.buf.drain(..pos + len).collect();
            let line = std::str::from_utf8(&line_bytes[..pos]).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::InvalidMessage,
                    "SSE stream is not valid UTF-8".to_string(),
                    false,
                )
            })?;
            if let Some(event) = self.process_line(line)? {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// Processes one line (WHATWG field rules); returns the event when the
    /// line is the blank-line dispatch. The per-frame data bound is
    /// enforced here (loud, never a truncated frame).
    fn process_line(&mut self, line: &str) -> Result<Option<SseEvent>, ProtocolError> {
        if line.is_empty() {
            // Dispatch: an event with no data lines is ignored (spec).
            if self.data_lines.is_empty() {
                self.event_field = None;
                return Ok(None);
            }
            let event = SseEvent {
                event: self.event_field.take(),
                data: self.data_lines.join("\n"),
            };
            self.data_lines.clear();
            self.data_bytes = 0;
            return Ok(Some(event));
        }
        if line.starts_with(':') {
            return Ok(None); // comment line
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => {
                let joined_growth = value.len() + usize::from(!self.data_lines.is_empty());
                if self.data_bytes + joined_growth > SSE_FRAME_MAX_BYTES {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidMessage,
                        format!(
                            "one SSE frame's joined data exceeded the {SSE_FRAME_MAX_BYTES}-byte \
                             frame bound; refusing (a runaway frame is a protocol failure, never \
                             a truncated parse)"
                        ),
                        false,
                    ));
                }
                self.data_bytes += joined_growth;
                self.data_lines.push(value.to_string());
            }
            "event" => self.event_field = Some(value.to_string()),
            // id/retry: deliberately ignored — the adapter never resumes a
            // stream, so last-event-id tracking and reconnect backoff have
            // no consumer (a truncated stream is a loud family error).
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_frames_dispatch_on_blank_lines() {
        let mut decoder = SseDecoder::new();
        let events = decoder
            .feed(b"data: one\n\ndata: two\nevent: thing\ndata: three\n\n")
            .expect("feed");
        // The event/data buffers accumulate until the blank-line dispatch:
        // "two" + the event field + "three" are ONE event (spec).
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: None,
                    data: "one".to_string()
                },
                SseEvent {
                    event: Some("thing".to_string()),
                    data: "two\nthree".to_string()
                },
            ]
        );
        let finish = decoder.finish().expect("finish");
        assert!(finish.events.is_empty());
        assert!(!finish.discarded_partial);
    }

    #[test]
    fn multi_line_data_joins_and_chunks_may_split_anywhere() {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        // Split mid-line, mid-CRLF and mid-multibyte.
        for chunk in [
            &b"dat"[..],
            b"a: fir",
            b"st\ndata: second\r",
            b"\n\r\n",
            "data: uni 真实\n\n".as_bytes(),
        ] {
            events.extend(decoder.feed(chunk).expect("feed"));
        }
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: None,
                    data: "first\nsecond".to_string()
                },
                SseEvent {
                    event: None,
                    data: "uni 真实".to_string()
                },
            ]
        );
    }

    #[test]
    fn comments_and_id_and_retry_fields_are_ignored() {
        let mut decoder = SseDecoder::new();
        let events = decoder
            .feed(b": a comment\nid: 42\nretry: 100\ndata: kept\n\n")
            .expect("feed");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "kept".to_string()
            }]
        );
    }

    #[test]
    fn an_unterminated_event_at_eof_is_discarded_and_reported() {
        let mut decoder = SseDecoder::new();
        let events = decoder
            .feed(b"data: complete\n\ndata: dangling")
            .expect("feed");
        assert_eq!(events.len(), 1);
        let finish = decoder.finish().expect("finish");
        assert!(finish.events.is_empty());
        assert!(
            finish.discarded_partial,
            "the dangling frame must be reported, never half-parsed"
        );
    }

    #[test]
    fn invalid_utf8_is_a_loud_error() {
        let mut decoder = SseDecoder::new();
        let err = decoder.feed(b"data: \xFF\xFE\n\n").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
    }

    #[test]
    fn the_buffer_bound_is_loud() {
        let mut decoder = SseDecoder::new();
        let huge = vec![b'x'; SSE_BUFFER_LIMIT + 1];
        let err = decoder.feed(&huge).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(err.message.contains("buffer bound"));
    }

    #[test]
    fn the_per_frame_bound_is_loud_across_lines_and_chunks() {
        // One frame whose JOINED data crosses the 1 MiB frame bound (split
        // over many lines and many feeds) is refused loudly — the whole-
        // stream buffer bound would not trip until 8 MiB.
        let mut decoder = SseDecoder::new();
        let quarter = "x".repeat(SSE_FRAME_MAX_BYTES / 4);
        for _ in 0..3 {
            decoder
                .feed(format!("data: {quarter}\n").as_bytes())
                .expect("the joined data so far fits the frame bound");
        }
        // 3 quarters + 2 newlines + 1 more quarter crosses 1 MiB.
        let err = decoder
            .feed(format!("data: {quarter}\n\n").as_bytes())
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidMessage);
        assert!(err.message.contains("frame bound"));
    }

    #[test]
    fn a_leading_bom_is_stripped_once() {
        let mut decoder = SseDecoder::new();
        let events = decoder.feed(b"\xEF\xBB\xBFdata: bom\n\n").expect("feed");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "bom".to_string()
            }]
        );
    }
}
