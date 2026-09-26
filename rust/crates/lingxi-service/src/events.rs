//! Event ordering, snapshots and resumable reads (R02-T05).
//!
//! Authority chain: key events are committed by the storage port inside
//! run transactions (R02-T04) and only exist on the `Ok` path
//! (`CommittedOutcome::events`). This module turns that durable log into
//! a subscription surface with three guarantees (taskbook 02 §7,
//! acceptances R02-A09 / R02-A10):
//!
//! 1. **Snapshot + subscription without a gap.** Registration and the
//!    durable snapshot cut are joined through the hub: a subscriber that
//!    registers while events keep committing buffers published events in a
//!    per-subscriber hold, the snapshot read pins an explicit boundary
//!    (`snapshotSeq` = highest committed seq included in the cut), and the
//!    hold is released by dropping everything `<= snapshotSeq` (it is in
//!    the snapshot) and delivering everything `> snapshotSeq` live. The
//!    single-writer seq assignment (MAX(seq)+1 inside the committing
//!    transaction on the one DB worker) is what makes this airtight: any
//!    event committed after the cut has `seq > snapshotSeq`; any event
//!    committed before it is in the cut; so the merged view is gap-free
//!    and the overlap is deduped by seq on the server side.
//! 2. **Explicit `snapshot_required` instead of silent replay.** A cursor
//!    whose events were truncated (storage floor > cursor + 1) is answered
//!    with a directive to rebuild from a snapshot — never an empty stream
//!    and never a silent replay from the wrong offset. Stale streams,
//!    malformed and future (forged) cursors are rejected loudly.
//! 3. **Bounded flow control without silent key-loss.** Every subscriber
//!    owns a bounded mailbox. Text deltas (`model_call_delta` /
//!    `assistant_segment_delta`) may be dropped under pressure (counted,
//!    bounded); a KEY event that cannot be enqueued detaches the
//!    subscription with an explicit `snapshot_required` signal (the client
//!    re-fetches a snapshot and resubscribes) — the server never silently
//!    drops a key result.
//!
//! Frame classes: business events delivered to subscribers are exactly the
//! frozen `EventEnvelope` of `lingxi.wire` v1 (one canonical JSON object
//! per WS text frame, PROTOCOL_SPEC §5) — this module never synthesizes
//! envelopes and never persists control traffic into `key_events`.
//! Subscription-layer control frames (`subscribed`, `snapshot_required`)
//! are a SEPARATE transport class: they carry `frameKind:"control"` and a
//! `type` tag and are distinguishable from envelopes by construction
//! (envelopes have neither a top-level `type` nor `frameKind` field).
//!
//! Determinism: no clocks and no timers inside the hub — ordering comes
//! from the durable seq plus a bounded reorder buffer, so tests drive
//! interleavings with fixed scheduling (single-thread runtimes + explicit
//! yields), never random sleeps.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use lingxi_kernel::ports::{EventStorePort, StorageError};
use lingxi_protocol::canon;
use lingxi_protocol::{Cursor, EventEnvelope, EventPayload, KnownEventPayload, Seq};

use base64::Engine as _;
use sha2::Digest as _;

use crate::auth::Principal;
use crate::sessions::{SessionAccess, SessionStore};

// ── Limits ─────────────────────────────────────────────────────────────────

/// Flow-control knobs of the event surface. Defaults are the production
/// values; tests inject small ones and drive overflow deterministically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLimits {
    /// Hard bound of one subscriber's mailbox (frames). Events may occupy
    /// `capacity - 1` slots; the remaining slot is reserved for the
    /// detach signal so a slow consumer can always be told to resync.
    pub subscriber_queue_capacity: usize,
    /// Bound of the per-stream reorder buffer. Concurrent committers may
    /// publish out of commit order; beyond this bound the stream is marked
    /// broken and its subscribers are detached (`snapshot_required`) — a
    /// publication gap is never papered over with unordered delivery.
    pub reorder_pending_bound: usize,
    /// Max events per snapshot/continuation page (HTTP and WS initial cut).
    pub page_limit_max: u32,
    /// Default page size when the caller does not ask.
    pub page_limit_default: u32,
}

impl Default for EventLimits {
    fn default() -> Self {
        Self {
            subscriber_queue_capacity: 128,
            reorder_pending_bound: 64,
            page_limit_max: 500,
            page_limit_default: 200,
        }
    }
}

impl EventLimits {
    pub fn validate(&self) -> Result<(), StorageError> {
        // Degenerate knobs are loud configuration errors, not silent
        // unboundedness.
        if self.subscriber_queue_capacity < 2 {
            return Err(StorageError::InvalidRequest {
                detail: "subscriber_queue_capacity must be >= 2 (one event slot + one \
                         reserved signal slot)"
                    .to_string(),
            });
        }
        if self.reorder_pending_bound == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "reorder_pending_bound must be >= 1 (0 would turn any concurrent \
                         commit into a broken stream)"
                    .to_string(),
            });
        }
        if self.page_limit_max == 0 || self.page_limit_default == 0 {
            return Err(StorageError::InvalidRequest {
                detail: "page limits must be >= 1 (0 would never return an event)".to_string(),
            });
        }
        if self.page_limit_default > self.page_limit_max {
            return Err(StorageError::InvalidRequest {
                detail: "page_limit_default must be <= page_limit_max".to_string(),
            });
        }
        Ok(())
    }
}

// ── Cursor codec ────────────────────────────────────────────────────────────

/// Domain tag mixed into the cursor checksum (versioned so a format change
/// cannot be confused with an old cursor).
const CURSOR_VERSION_TAG: &str = "lingxi-events-cursor-v1";

/// Server-side interpretation of a subscription cursor. The wire form is
/// an opaque string (protocol `Cursor`); THIS decoder is the only reader.
///
/// The checksum is INTEGRITY (detects accidental corruption and blind
/// forging), not authenticity — authority always comes from the
/// server-side checks in [`EventService::subscribe`] (stream binding, seq
/// bounds against the durable head, gap detection against the durable
/// floor). Documented deliberately so it is never mistaken for an HMAC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscribeCursor {
    pub stream_id: String,
    pub seq: Seq,
}

fn cursor_checksum(stream_id: &str, seq: u64) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(CURSOR_VERSION_TAG.as_bytes());
    hasher.update(b"|");
    hasher.update(stream_id.as_bytes());
    hasher.update(b"|");
    hasher.update(seq.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

impl SubscribeCursor {
    /// Encodes the cursor for a client (issued only from a real cut of
    /// the durable log).
    pub fn encode(&self) -> Cursor {
        let body = serde_json::json!({
            "chk": cursor_checksum(&self.stream_id, self.seq.value()),
            "q": self.seq.value(),
            "s": self.stream_id,
        });
        // Canonical JSON (sorted keys) → base64url without padding: the
        // cursor stays a stable, opaque ASCII token.
        let raw = canon::canonical_string(&body);
        Cursor::new(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes()))
    }

    /// Strict decode: base64url → JSON object with exactly the expected
    /// keys → checksum re-verification. Any deviation is malformed —
    /// never a guess.
    pub fn decode(cursor: &Cursor) -> Result<Self, String> {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor.as_str().as_bytes())
            .map_err(|_| "cursor is not valid base64url".to_string())?;
        let text = String::from_utf8(raw).map_err(|_| "cursor bytes are not UTF-8".to_string())?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|err| format!("cursor body is not JSON: {err}"))?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            chk: String,
            q: u64,
            s: String,
        }
        let body: Body = serde_json::from_value(value)
            .map_err(|err| format!("cursor body has an unexpected shape: {err}"))?;
        if cursor_checksum(&body.s, body.q) != body.chk {
            return Err("cursor checksum mismatch (corrupted or forged)".to_string());
        }
        Ok(Self {
            stream_id: body.s,
            seq: Seq::new(body.q),
        })
    }
}

// ── Subscriber mailbox (bounded, one reserved signal slot) ──────────────────

/// One frame delivered to a subscriber.
#[derive(Debug, Clone, PartialEq)]
pub enum SubscriptionFrame {
    /// A business event (frozen envelope, exactly as committed). Boxed:
    /// the envelope dominates the variant size and frames move through
    /// the bounded mailbox by value.
    Event(Box<EventEnvelope>),
    /// The subscription is detached; the client must rebuild from a
    /// snapshot and resubscribe.
    SnapshotRequired { reason: DetachReason },
}

/// Why a subscription was detached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachReason {
    /// The subscriber's bounded mailbox could not take a KEY event.
    SlowConsumer,
    /// The reorder buffer overflowed: a published seq gap exceeded the
    /// pending bound, so ordering can no longer be proven.
    PublicationGap,
}

impl DetachReason {
    pub fn wire_reason(&self) -> &'static str {
        match self {
            DetachReason::SlowConsumer => "slow_consumer",
            DetachReason::PublicationGap => "publication_gap",
        }
    }
}

struct MailboxInner {
    queue: VecDeque<SubscriptionFrame>,
    capacity: usize,
    closed: bool,
}

/// Bounded FIFO mailbox shared between publishers (sync, under the hub
/// lock) and exactly one consumer (the WS forwarder / tests).
pub struct Mailbox {
    inner: Mutex<MailboxInner>,
    notify: tokio::sync::Notify,
}

impl Mailbox {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(MailboxInner {
                queue: VecDeque::new(),
                capacity,
                closed: false,
            }),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// Enqueues an EVENT frame. `false` (overflow) when the queue already
    /// holds `capacity - 1` frames — one slot stays reserved for the
    /// detach signal — or when the consumer is gone.
    fn push_event(&self, frame: SubscriptionFrame) -> bool {
        let mut inner = self.lock();
        if inner.closed || inner.queue.len() + 1 >= inner.capacity {
            return false;
        }
        inner.queue.push_back(frame);
        drop(inner);
        self.notify.notify_waiters();
        true
    }

    /// Enqueues the DETACH signal (may use the reserved slot). `false`
    /// only when the consumer is gone or the queue is truly full.
    fn push_signal(&self, frame: SubscriptionFrame) -> bool {
        let mut inner = self.lock();
        if inner.closed || inner.queue.len() >= inner.capacity {
            return false;
        }
        inner.queue.push_back(frame);
        drop(inner);
        self.notify.notify_waiters();
        true
    }

    fn close(&self) {
        self.lock().closed = true;
        self.notify.notify_waiters();
    }

    fn is_closed(&self) -> bool {
        self.lock().closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MailboxInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Non-blocking receive (tests + forwarder fast path).
    pub fn try_recv(&self) -> Option<SubscriptionFrame> {
        self.lock().queue.pop_front()
    }

    /// Async receive; `None` once closed AND drained.
    pub async fn recv(&self) -> Option<SubscriptionFrame> {
        loop {
            // Register interest BEFORE checking so a notify racing between
            // the check and the await cannot be lost.
            let notified = self.notify.notified();
            if let Some(frame) = self.try_recv() {
                return Some(frame);
            }
            if self.is_closed() {
                return None;
            }
            notified.await;
        }
    }
}

// ── Hub ─────────────────────────────────────────────────────────────────────

/// Sentinel cut while the durable snapshot is being taken.
const HOLD_ALL: u64 = u64::MAX;

struct SubscriberState {
    id: u64,
    stream_id: String,
    mailbox: Arc<Mailbox>,
    /// Mailbox capacity recorded at registration (bounds the hold).
    queue_capacity: usize,
    /// Skip live events with `seq <= cut` (they are in this subscriber's
    /// durable cut). `HOLD_ALL` while the cut is being taken.
    cut: u64,
    /// Events buffered between registration and the snapshot cut.
    hold: Vec<EventEnvelope>,
    /// Highest seq accounted for this subscriber (server-side dedup).
    /// Two roles: while `cut == HOLD_ALL` it advances with every buffered
    /// hold event (hold-dedup); [`EventHub::release_hold`] re-anchors it at
    /// the cut before flushing, after which it advances only on actual
    /// live delivery (duplicate dedup).
    last_enqueued_seq: u64,
    detached: Option<DetachReason>,
    /// Diagnostics: lossy deltas dropped by the overflow policy.
    dropped_deltas: u64,
}

struct StreamState {
    /// `false` until the first publication after process start; the first
    /// published seq becomes the baseline (a restarted process resumes
    /// mid-stream — contiguity is relative, not absolute).
    initialized: bool,
    /// Next seq expected from publishers (last delivered + 1).
    next_seq: u64,
    /// Out-of-commit-order publications waiting for their predecessor.
    pending: BTreeMap<u64, EventEnvelope>,
    /// Reorder bound exceeded: published order can no longer be proven.
    broken: bool,
}

struct HubInner {
    streams: HashMap<String, StreamState>,
    subscribers: HashMap<u64, SubscriberState>,
    next_subscriber_id: u64,
}

/// The in-memory publication hub: per-stream ordering, dedup, bounded
/// fan-out. Every method takes the lock briefly and never holds it across
/// an await; publication itself is synchronous (bounded try-ops only).
pub struct EventHub {
    inner: Mutex<HubInner>,
    limits: EventLimits,
}

impl EventHub {
    pub fn new(limits: EventLimits) -> Self {
        Self {
            inner: Mutex::new(HubInner {
                streams: HashMap::new(),
                subscribers: HashMap::new(),
                next_subscriber_id: 0,
            }),
            limits,
        }
    }

    /// Publishes committed events (called strictly AFTER the storage port
    /// returned `Ok`). Per stream this is idempotent (duplicates by seq —
    /// equivalently eventId — are skipped), ordered (a bounded reorder
    /// buffer repairs commit-order/publish-order inversions) and lossless
    /// for key events (overflow detaches the subscriber with an explicit
    /// signal).
    pub fn publish(&self, envelopes: &[EventEnvelope]) {
        let mut inner = self.hub_lock();
        for envelope in envelopes {
            let stream_key = envelope.stream_id.to_string();
            let seq = envelope.seq.value();
            if !inner.streams.contains_key(&stream_key) {
                inner.streams.insert(
                    stream_key.clone(),
                    StreamState {
                        initialized: false,
                        next_seq: 0,
                        pending: BTreeMap::new(),
                        broken: false,
                    },
                );
            }
            if inner.streams.get(&stream_key).is_some_and(|s| s.broken) {
                // Storage stays authoritative; subscribers were detached.
                tracing::warn!(
                    stream = %envelope.stream_id,
                    seq = %envelope.seq,
                    "event published on a broken stream (earlier publication gap); \
                     subscribers must resync from storage"
                );
                continue;
            }
            {
                let stream = inner
                    .streams
                    .get_mut(&stream_key)
                    .expect("stream state just inserted");
                if !stream.initialized {
                    // Absolute contiguity: a fresh stream starts at seq 1.
                    // (A restarted process aligns the baseline at
                    // registration instead — see `register`.)
                    stream.initialized = true;
                    stream.next_seq = 1;
                }
                if seq < stream.next_seq {
                    // Duplicate publication (idempotent replay of an
                    // already-published commit).
                    continue;
                }
                if seq > stream.next_seq {
                    if stream.pending.contains_key(&seq) {
                        continue; // exact duplicate parked earlier
                    }
                    if stream.pending.len() >= self.limits.reorder_pending_bound {
                        // Ordering can no longer be proven: detach every
                        // subscriber of this stream instead of delivering
                        // unordered events.
                        stream.pending.clear();
                        stream.broken = true;
                        let ids: Vec<u64> = inner
                            .subscribers
                            .values()
                            .filter(|s| s.stream_id == stream_key)
                            .map(|s| s.id)
                            .collect();
                        for id in ids {
                            detach_subscriber(&mut inner, id, DetachReason::PublicationGap);
                        }
                        tracing::error!(
                            stream = %envelope.stream_id,
                            bound = self.limits.reorder_pending_bound,
                            "event publication gap exceeded the reorder bound; \
                             stream marked broken"
                        );
                        continue;
                    }
                    stream.pending.insert(seq, envelope.clone());
                    continue;
                }
            }
            // seq == next_seq: deliver, then drain the reorder buffer.
            self.deliver_next(&mut inner, &stream_key, envelope.clone());
            loop {
                let next_seq = inner
                    .streams
                    .get(&stream_key)
                    .expect("stream state inserted above under the same lock")
                    .next_seq;
                let Some(parked) = inner
                    .streams
                    .get_mut(&stream_key)
                    .expect("stream state inserted above under the same lock")
                    .pending
                    .remove(&next_seq)
                else {
                    break;
                };
                self.deliver_next(&mut inner, &stream_key, parked);
            }
        }
    }

    /// Delivers `envelope` (whose seq equals the stream's `next_seq`) to
    /// the stream's subscribers and advances `next_seq`.
    ///
    /// The `expect`s are reasoned invariants, not guesses: every caller
    /// inserts the stream state in the same locked section before calling,
    /// so a miss would be a logic bug that must fail loudly (never a
    /// silently skipped publication).
    fn deliver_next(&self, inner: &mut HubInner, stream_key: &str, envelope: EventEnvelope) {
        let seq = envelope.seq.value();
        inner
            .streams
            .get_mut(stream_key)
            .expect("caller inserted the stream state under the same lock")
            .next_seq = seq + 1;
        let ids: Vec<u64> = inner
            .subscribers
            .values()
            .filter(|s| s.stream_id == stream_key)
            .map(|s| s.id)
            .collect();
        for id in ids {
            self.enqueue_for(inner, id, envelope.clone(), seq);
        }
    }

    /// Single home of the delivery rules: hold during the snapshot cut,
    /// overlap dedup by cut, duplicate dedup by seq, delta-overflow drop
    /// vs key-overflow detach, dead-consumer pruning.
    fn enqueue_for(&self, inner: &mut HubInner, id: u64, envelope: EventEnvelope, seq: u64) {
        let Some(subscriber) = inner.subscribers.get_mut(&id) else {
            return;
        };
        if subscriber.detached.is_some() {
            return;
        }
        if subscriber.cut != HOLD_ALL && seq <= subscriber.cut {
            // Already in this subscriber's durable cut (overlap dedup).
            subscriber.last_enqueued_seq = subscriber.last_enqueued_seq.max(seq);
            return;
        }
        if seq <= subscriber.last_enqueued_seq {
            return; // duplicate by seq (eventId is unique per durable row)
        }
        if subscriber.cut == HOLD_ALL {
            // Snapshot cut in flight: buffer in the hold. The hold is
            // bounded like the mailbox (capacity - 1 event slots); an
            // over-long cut window detaches exactly like a slow consumer.
            if subscriber.hold.len() + 1 >= subscriber.queue_capacity {
                let mailbox = Arc::clone(&subscriber.mailbox);
                detach_with_signal(inner, id, mailbox, DetachReason::SlowConsumer);
                return;
            }
            subscriber.hold.push(envelope);
            subscriber.last_enqueued_seq = seq;
            return;
        }
        let mailbox = Arc::clone(&subscriber.mailbox);
        let is_delta = is_lossy_delta(&envelope.payload);
        if mailbox.push_event(SubscriptionFrame::Event(Box::new(envelope))) {
            subscriber.last_enqueued_seq = seq;
            return;
        }
        if mailbox.is_closed() {
            // Consumer gone: unregister now (the guard's Drop also closes;
            // this is the publish-side prune).
            if let Some(removed) = inner.subscribers.remove(&id) {
                removed.mailbox.close();
            }
            return;
        }
        if is_delta {
            // Bounded, counted loss of a text increment; key results are
            // never dropped silently (the branch below detaches).
            if let Some(subscriber) = inner.subscribers.get_mut(&id) {
                subscriber.dropped_deltas += 1;
            }
            return;
        }
        detach_with_signal(inner, id, mailbox, DetachReason::SlowConsumer);
    }

    /// Registers a subscriber on `stream_id` starting after `after_seq`.
    /// `known_head` is the durable head the caller read just before
    /// registering (used for restart baseline alignment — see below). The
    /// returned guard is in the HOLD state until
    /// [`EventHub::release_hold`] pins the durable cut.
    fn register(
        &self,
        stream_id: &str,
        after_seq: u64,
        known_head: Option<u64>,
    ) -> SubscriptionGuard {
        let mut inner = self.hub_lock();
        let id = inner.next_subscriber_id;
        inner.next_subscriber_id += 1;
        // Baseline alignment for a stream this process has not delivered
        // from yet (restart resume). Safe ONLY while the stream has zero
        // subscribers: any event `<= known_head` is durably committed, and
        // every subscriber registered from now on takes its cut from a
        // storage read that includes it — so skipping such seqs live loses
        // nothing. With live subscribers the normal delivery flow already
        // owns `next_seq`.
        if let Some(known_head) = known_head {
            let has_subscribers = inner.subscribers.values().any(|s| s.stream_id == stream_id);
            if !has_subscribers {
                let stream = inner
                    .streams
                    .entry(stream_id.to_string())
                    .or_insert_with(|| StreamState {
                        initialized: false,
                        next_seq: 0,
                        pending: BTreeMap::new(),
                        broken: false,
                    });
                if !stream.broken && stream.next_seq <= known_head {
                    stream.initialized = true;
                    stream.next_seq = known_head + 1;
                    // Parked publications at or below the new baseline are
                    // covered by every future cut; drop them as garbage.
                    stream.pending.retain(|seq, _| *seq > known_head);
                    // A parked publication exactly AT the new baseline is
                    // the next event the hub owes: leaving it parked would
                    // stall the stream forever, because its drain depends
                    // on a smaller seq arriving first and the baseline
                    // guarantees none will (REVIEW-R1 F02). Drain the whole
                    // contiguous chain the alignment makes deliverable.
                    // Safety: every drained event was published, hence
                    // committed before this registration — there are no
                    // subscribers to deliver it to (alignment only runs for
                    // subscriber-less streams) and every durable cut taken
                    // from now on covers it (a page-capped cut covers it
                    // through `next_cursor` pagination), so advancing past
                    // it here loses nothing and unblocks the stream.
                    while stream.pending.contains_key(&stream.next_seq) {
                        stream.pending.remove(&stream.next_seq);
                        stream.next_seq += 1;
                    }
                }
            }
        }
        let mailbox = Arc::new(Mailbox::new(self.limits.subscriber_queue_capacity));
        inner.subscribers.insert(
            id,
            SubscriberState {
                id,
                stream_id: stream_id.to_string(),
                mailbox: Arc::clone(&mailbox),
                queue_capacity: self.limits.subscriber_queue_capacity,
                cut: HOLD_ALL,
                hold: Vec::new(),
                last_enqueued_seq: after_seq,
                detached: None,
                dropped_deltas: 0,
            },
        );
        SubscriptionGuard {
            subscriber_id: id,
            mailbox,
            hub: None,
        }
    }

    /// Completes the registration: everything `<= cut` was in the durable
    /// cut; buffered events above it flush in order; live delivery
    /// continues from the cut.
    fn release_hold(&self, subscriber_id: u64, cut: u64) {
        let mut inner = self.hub_lock();
        let Some(subscriber) = inner.subscribers.get_mut(&subscriber_id) else {
            return;
        };
        let held: Vec<EventEnvelope> = std::mem::take(&mut subscriber.hold);
        subscriber.cut = cut;
        // Re-anchor the dedup watermark AT the cut before flushing. While
        // the hold was open, every buffered event advanced
        // `last_enqueued_seq` (its hold-dedup role), so the watermark here
        // is `max(after_seq, highest held seq)` — replaying the held tail
        // through `enqueue_for` without the re-anchor would hit the
        // duplicate check (`seq <= last_enqueued_seq`) and silently drop
        // every held event above the cut (REVIEW-R1 F01). After the
        // re-anchor the watermark means exactly "everything <= cut is
        // accounted for by the durable snapshot"; the flush below
        // re-advances it as it delivers, and live delivery continues from
        // there. No duplicate is possible: held events are delivered once
        // here, and anything published after this point has
        // `seq >= stream.next_seq > highest held seq`.
        //
        // Callers contract: `cut >= after_seq` (the cut is the boundary of
        // a durable read starting after `after_seq`), so the re-anchor
        // never moves the watermark below the resume point. The whole
        // operation runs under the hub lock, so no publisher can observe
        // the intermediate watermark.
        subscriber.last_enqueued_seq = cut;
        // The hold only ever contains hub-ordered (ascending) events;
        // flush the tail above the cut in order.
        for envelope in held {
            let seq = envelope.seq.value();
            if seq <= cut {
                continue;
            }
            self.enqueue_for(&mut inner, subscriber_id, envelope, seq);
        }
    }

    fn unregister(&self, subscriber_id: u64) {
        let mut inner = self.hub_lock();
        if let Some(removed) = inner.subscribers.remove(&subscriber_id) {
            removed.mailbox.close();
        }
    }

    /// Diagnostics of one live subscription (evidence / tests). `None`
    /// after the subscription ended.
    pub fn subscriber_stats(&self, subscriber_id: u64) -> Option<SubscriberStats> {
        let inner = self.hub_lock();
        inner
            .subscribers
            .get(&subscriber_id)
            .map(|s| SubscriberStats {
                stream_id: s.stream_id.clone(),
                last_enqueued_seq: Seq::new(s.last_enqueued_seq),
                dropped_deltas: s.dropped_deltas,
                detached: s.detached.clone(),
            })
    }

    fn hub_lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Observable state of one subscription (diagnostics / evidence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriberStats {
    pub stream_id: String,
    pub last_enqueued_seq: Seq,
    pub dropped_deltas: u64,
    pub detached: Option<DetachReason>,
}

/// Consumer-side handle; dropping it unregisters the subscription.
pub struct SubscriptionGuard {
    subscriber_id: u64,
    mailbox: Arc<Mailbox>,
    hub: Option<Arc<EventHub>>,
}

impl SubscriptionGuard {
    pub fn mailbox(&self) -> &Arc<Mailbox> {
        &self.mailbox
    }

    pub fn subscriber_id(&self) -> u64 {
        self.subscriber_id
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        match self.hub.take() {
            Some(hub) => hub.unregister(self.subscriber_id),
            None => self.mailbox.close(),
        }
    }
}

fn detach_with_signal(inner: &mut HubInner, id: u64, mailbox: Arc<Mailbox>, reason: DetachReason) {
    let Some(subscriber) = inner.subscribers.get_mut(&id) else {
        return;
    };
    if subscriber.detached.is_some() {
        return;
    }
    subscriber.detached = Some(reason.clone());
    // The reserved slot makes this deliverable even under pressure; if the
    // consumer is already gone there is nothing to signal.
    let _ = mailbox.push_signal(SubscriptionFrame::SnapshotRequired { reason });
}

fn detach_subscriber(inner: &mut HubInner, id: u64, reason: DetachReason) {
    let Some(subscriber) = inner.subscribers.get_mut(&id) else {
        return;
    };
    let mailbox = Arc::clone(&subscriber.mailbox);
    detach_with_signal(inner, id, mailbox, reason);
}

/// Text increments that may be lost under bounded backpressure. Everything
/// else (run state changes, tool results, final messages, segment
/// boundaries) is a key event.
fn is_lossy_delta(payload: &EventPayload) -> bool {
    matches!(
        payload,
        EventPayload::Known(KnownEventPayload::ModelCallDelta(_))
            | EventPayload::Known(KnownEventPayload::AssistantSegmentDelta(_))
    )
}

// ── EventService: ownership + snapshot/cursor protocol ─────────────────────

/// The committed cut handed to a subscriber (and to the HTTP snapshot
/// endpoint): events after `from_seq` up to and including `snapshot_seq`,
/// plus the continuation cursor when the page limit cut the read short.
#[derive(Debug, Clone, PartialEq)]
pub struct EventCut {
    pub stream_id: String,
    /// `snapshot` (fresh rebuild from the floor) or `resume` (valid cursor
    /// continuation).
    pub mode: &'static str,
    /// First seq this cut starts AFTER.
    pub from_seq: Seq,
    /// Explicit boundary: highest committed seq included.
    pub snapshot_seq: Seq,
    pub events: Vec<EventEnvelope>,
    /// Set when more committed events exist beyond the page; the client
    /// paginates with it and dedups the bounded overlap by seq.
    pub next_cursor: Option<Cursor>,
}

/// The directive returned when a cursor cannot be resumed.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRequired {
    pub stream_id: String,
    /// Lowest retained seq (None when the stream holds no events at all).
    pub floor: Option<Seq>,
    pub reason: &'static str,
}

/// Outcome of a subscribe request. Not `Clone`/`Debug`: the `Started`
/// arm owns the live subscription (exactly one consumer).
pub enum SubscribeOutcome {
    Started {
        cut: EventCut,
        subscription: SubscriptionGuard,
    },
    RequiresSnapshot(SnapshotRequired),
}

impl std::fmt::Debug for SubscribeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscribeOutcome::Started { cut, .. } => f
                .debug_struct("Started")
                .field("cut", cut)
                .finish_non_exhaustive(),
            SubscribeOutcome::RequiresSnapshot(required) => {
                f.debug_tuple("RequiresSnapshot").field(required).finish()
            }
        }
    }
}

/// Loud rejections of a subscribe request (mapped to protocol errors by
/// the transport; none is ever a silent fallback).
#[derive(Debug, Clone, PartialEq)]
pub enum SubscribeReject {
    /// The stream does not exist (stale/unknown stream id).
    StreamNotFound {
        stream_id: String,
    },
    /// The stream belongs to another principal.
    Forbidden {
        stream_id: String,
    },
    /// Cursor failed strict decoding (malformed / corrupted / forged).
    MalformedCursor {
        detail: String,
    },
    /// Cursor was issued for a different stream.
    StaleStreamCursor {
        stream_id: String,
        cursor_stream: String,
    },
    /// Cursor points beyond the durable head (forged or future).
    FutureCursor {
        stream_id: String,
        seq: Seq,
        head: Seq,
    },
    Storage(StorageError),
}

impl SubscribeReject {
    /// Machine reason code for the transport error surface.
    pub fn reason_code(&self) -> &'static str {
        match self {
            SubscribeReject::StreamNotFound { .. } => "stream_not_found",
            SubscribeReject::Forbidden { .. } => "cross_principal_access",
            SubscribeReject::MalformedCursor { .. } => "malformed_cursor",
            SubscribeReject::StaleStreamCursor { .. } => "stale_stream_cursor",
            SubscribeReject::FutureCursor { .. } => "future_cursor",
            SubscribeReject::Storage(_) => "db_failure",
        }
    }
}

/// The event subscription service: snapshot + cursor protocol over the
/// durable log, live fan-out through the hub, publication entry point for
/// the post-commit path.
///
/// R02 stream namespace: one stream per session (`streamId == sessionId`),
/// which is how the R02-T04 writer assigns `stream_id`. The API is
/// stream-keyed throughout so finer-grained streams (per attempt) can be
/// added later without changing the protocol surface.
pub struct EventService {
    hub: Arc<EventHub>,
    storage: Arc<lingxi_adapters::storage::RunDatabase>,
    sessions: Arc<SessionStore>,
    limits: EventLimits,
}

impl EventService {
    pub fn new(
        storage: Arc<lingxi_adapters::storage::RunDatabase>,
        sessions: Arc<SessionStore>,
        limits: EventLimits,
    ) -> Result<Self, StorageError> {
        limits.validate()?;
        Ok(Self {
            hub: Arc::new(EventHub::new(limits.clone())),
            storage,
            sessions,
            limits,
        })
    }

    /// Post-commit publication entry point (R02-T05 wiring of the T04
    /// authority chain). Callers hand over `CommittedOutcome::events`
    /// strictly after the storage port returned `Ok`.
    pub fn publish_committed(&self, envelopes: &[EventEnvelope]) {
        self.hub.publish(envelopes);
    }

    pub fn limits(&self) -> &EventLimits {
        &self.limits
    }

    pub fn hub(&self) -> &Arc<EventHub> {
        &self.hub
    }

    /// Subscribes to `stream_id` (ownership-checked through the SAME
    /// session rule as every other read), optionally resuming from a
    /// cursor issued by a previous cut.
    ///
    /// Snapshot/cursor join (module docs guarantee #1): register (hold) →
    /// durable read → release hold at the read boundary.
    pub async fn subscribe(
        &self,
        principal: &Principal,
        stream_id: &str,
        cursor: Option<Cursor>,
    ) -> Result<SubscribeOutcome, SubscribeReject> {
        // 1. Ownership first: in R02 a stream id IS a session id; unknown
        //    streams are stale-stream rejections, foreign streams are
        //    forbidden — never a peek.
        match self.sessions.get_for(principal, stream_id).await {
            Ok(SessionAccess::Ok(_)) => {}
            Ok(SessionAccess::NotFound) => {
                return Err(SubscribeReject::StreamNotFound {
                    stream_id: stream_id.to_string(),
                })
            }
            Ok(SessionAccess::Forbidden) => {
                return Err(SubscribeReject::Forbidden {
                    stream_id: stream_id.to_string(),
                })
            }
            Err(err) => return Err(SubscribeReject::Storage(err)),
        }

        // 2. Durable head (always read: cursor bounds AND the hub's
        //    restart baseline alignment both need it).
        let head = self
            .storage
            .stream_head(stream_id)
            .await
            .map_err(SubscribeReject::Storage)?
            .unwrap_or(Seq::new(0));

        // 3. Cursor validation against the durable facts.
        let after = match &cursor {
            None => Seq::new(0),
            Some(cursor) => {
                let decoded = SubscribeCursor::decode(cursor)
                    .map_err(|detail| SubscribeReject::MalformedCursor { detail })?;
                if decoded.stream_id != stream_id {
                    return Err(SubscribeReject::StaleStreamCursor {
                        stream_id: stream_id.to_string(),
                        cursor_stream: decoded.stream_id,
                    });
                }
                let floor = self
                    .storage
                    .stream_floor(stream_id)
                    .await
                    .map_err(SubscribeReject::Storage)?;
                // Truncation is checked against the RETAINED facts before
                // the future bound. Gap = events the cursor expects were
                // truncated: the explicit rebuild directive, not an empty
                // stream, not a replay from the wrong offset. A collapsed
                // stream (no floor at all — retention purged it empty) with
                // a non-zero cursor is the same condition: its events are
                // gone, so the client gets the rebuild directive with no
                // floor instead of a "future cursor" rejection that would
                // send it down the wrong recovery path (REVIEW-R1 F03 —
                // R02-A10 recovery vocabulary).
                match floor {
                    Some(floor) if floor.value() > decoded.seq.value().saturating_add(1) => {
                        return Ok(SubscribeOutcome::RequiresSnapshot(SnapshotRequired {
                            stream_id: stream_id.to_string(),
                            floor: Some(floor),
                            reason: "events_truncated",
                        }));
                    }
                    // No retained events at all, cursor expects some.
                    None if decoded.seq.value() > 0 => {
                        return Ok(SubscribeOutcome::RequiresSnapshot(SnapshotRequired {
                            stream_id: stream_id.to_string(),
                            floor: None,
                            reason: "events_truncated",
                        }));
                    }
                    _ => {}
                }
                if decoded.seq > head {
                    return Err(SubscribeReject::FutureCursor {
                        stream_id: stream_id.to_string(),
                        seq: decoded.seq,
                        head,
                    });
                }
                decoded.seq
            }
        };

        // 4. Register (hold) BEFORE the durable page read: events published
        //    from now on buffer in the hold instead of racing the cut.
        let mut subscription = self
            .hub
            .register(stream_id, after.value(), Some(head.value()));
        let page_limit = u64::from(self.limits.page_limit_max);
        let events = match self
            .storage
            .stream_events_after(stream_id, after, self.limits.page_limit_max)
            .await
        {
            Ok(events) => events,
            Err(err) => {
                // Undo the registration explicitly (the guard alone would
                // only close the mailbox; the hub entry must not linger).
                self.hub.unregister(subscription.subscriber_id());
                return Err(SubscribeReject::Storage(err));
            }
        };
        let snapshot_seq = events.last().map_or(after, |e| e.seq);
        let next_cursor = if events.len() as u64 >= page_limit {
            Some(
                SubscribeCursor {
                    stream_id: stream_id.to_string(),
                    seq: snapshot_seq,
                }
                .encode(),
            )
        } else {
            None
        };
        let mode = if cursor.is_some() {
            "resume"
        } else {
            "snapshot"
        };
        // 4. Release the hold AT the cut boundary (module docs guarantee).
        self.hub
            .release_hold(subscription.subscriber_id(), snapshot_seq.value());
        subscription.hub = Some(Arc::clone(&self.hub));
        Ok(SubscribeOutcome::Started {
            cut: EventCut {
                stream_id: stream_id.to_string(),
                mode,
                from_seq: after,
                snapshot_seq,
                events,
                next_cursor,
            },
            subscription,
        })
    }

    /// HTTP snapshot/continuation page (same authority and cursor
    /// semantics as the WS path; errors carry `cursor_expired` for the
    /// rebuild directive).
    pub async fn events_page(
        &self,
        principal: &Principal,
        stream_id: &str,
        cursor: Option<Cursor>,
        limit: Option<u32>,
    ) -> Result<EventCut, SubscribePageError> {
        let limit = limit.unwrap_or(self.limits.page_limit_default);
        if limit == 0 || limit > self.limits.page_limit_max {
            return Err(SubscribePageError::InvalidLimit {
                requested: limit,
                max: self.limits.page_limit_max,
            });
        }
        match self.subscribe(principal, stream_id, cursor).await {
            Ok(SubscribeOutcome::Started { mut cut, .. }) => {
                // Re-cut to the requested (possibly smaller) page size.
                if (cut.events.len() as u32) > limit {
                    cut.events.truncate(limit as usize);
                    let last = cut.events.last().map_or(cut.from_seq, |e| e.seq);
                    cut.next_cursor = Some(
                        SubscribeCursor {
                            stream_id: stream_id.to_string(),
                            seq: last,
                        }
                        .encode(),
                    );
                    cut.snapshot_seq = last;
                }
                Ok(cut)
            }
            // A page request with an expired cursor is the canonical
            // `cursor_expired` (client refetches the snapshot).
            Ok(SubscribeOutcome::RequiresSnapshot(required)) => {
                Err(SubscribePageError::CursorExpired(required))
            }
            Err(reject) => Err(SubscribePageError::Reject(reject)),
        }
    }

    /// Head of a stream (diagnostics/tests; the same durable read the
    /// subscribe path uses).
    pub async fn stream_head(&self, stream_id: &str) -> Result<Option<Seq>, StorageError> {
        self.storage.stream_head(stream_id).await
    }

    /// Retention maintenance passthrough (creates the R02-A10 precondition
    /// through the real product surface; see the port docs).
    pub async fn purge_events_before(
        &self,
        stream_id: &str,
        before_seq: Seq,
    ) -> Result<u64, StorageError> {
        self.storage
            .purge_events_before(stream_id, before_seq)
            .await
    }
}

/// Errors of the HTTP page endpoint (subscribe rejections + page-specific
/// conditions).
#[derive(Debug, Clone, PartialEq)]
pub enum SubscribePageError {
    InvalidLimit {
        requested: u32,
        max: u32,
    },
    /// The explicit rebuild directive over HTTP.
    CursorExpired(SnapshotRequired),
    Reject(SubscribeReject),
}

// ── WS control frames (subscription layer, NOT lingxi.wire events) ─────────

/// Builds the `subscribed` control frame (canonical JSON).
pub fn control_subscribed_json(cut: &EventCut) -> String {
    let mut body = serde_json::json!({
        "frameKind": "control",
        "type": "subscribed",
        "streamId": cut.stream_id,
        "mode": cut.mode,
        "snapshotSeq": cut.snapshot_seq.to_wire_string(),
        "fromSeq": cut.from_seq.to_wire_string(),
    });
    if let Some(next) = &cut.next_cursor {
        body["nextCursor"] = serde_json::Value::String(next.as_str().to_string());
    }
    canon::canonical_string(&body)
}

/// Builds the `snapshot_required` control frame (canonical JSON). Used for
/// BOTH the expired-cursor directive and the detach signal.
pub fn control_snapshot_required_json(stream_id: &str, floor: Option<Seq>, reason: &str) -> String {
    let mut body = serde_json::json!({
        "frameKind": "control",
        "type": "snapshot_required",
        "streamId": stream_id,
        "reason": reason,
    });
    if let Some(floor) = floor {
        body["floorSeq"] = serde_json::Value::String(floor.to_wire_string());
    }
    canon::canonical_string(&body)
}

/// Serializes a detach frame delivered through a subscription mailbox.
pub fn control_frame_of_detach(stream_id: &str, reason: &DetachReason) -> String {
    control_snapshot_required_json(stream_id, None, reason.wire_reason())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::{
        AssistantPhase, AttemptId, EventId, KnownEventPayload, ModelCallDeltaPayload,
        RunStateChangedPayload, RunStatus, SessionId, StreamId,
    };

    fn envelope(stream: &str, seq: u64, delta: bool) -> EventEnvelope {
        let payload = if delta {
            EventPayload::Known(KnownEventPayload::ModelCallDelta(ModelCallDeltaPayload {
                model_call_id: lingxi_protocol::ModelCallId::new(format!("mc-{seq}")),
                phase: AssistantPhase::FinalAnswer,
                delta: "x".to_string(),
            }))
        } else {
            EventPayload::Known(KnownEventPayload::RunStateChanged(RunStateChangedPayload {
                from: RunStatus::Running,
                to: RunStatus::Completed,
                reason: None,
            }))
        };
        EventEnvelope::new(
            EventId::new(format!("evt-{stream}-{seq}")),
            StreamId::new(stream.to_string()),
            Seq::new(seq),
            SessionId::new(stream.to_string()),
            Some(lingxi_protocol::RunId::new(format!("run-{seq}"))),
            Some(AttemptId::new(format!("run-{seq}#a1"))),
            payload,
        )
    }

    fn limits(capacity: usize, pending: usize) -> EventLimits {
        EventLimits {
            subscriber_queue_capacity: capacity,
            reorder_pending_bound: pending,
            page_limit_max: 50,
            page_limit_default: 50,
        }
    }

    #[test]
    fn cursor_codec_roundtrip_and_strict_rejection() {
        let cursor = SubscribeCursor {
            stream_id: "sess_a".to_string(),
            seq: Seq::new(42),
        }
        .encode();
        let back = SubscribeCursor::decode(&cursor).expect("roundtrip");
        assert_eq!(back.stream_id, "sess_a");
        assert_eq!(back.seq, Seq::new(42));

        // Garbage, wrong padding, tampered body.
        for bad in [
            Cursor::new("not-base64!!!"),
            Cursor::new(""),
            Cursor::new("aGVsbG8gd29ybGQ"), // valid b64url, not our JSON
        ] {
            assert!(
                SubscribeCursor::decode(&bad).is_err(),
                "must reject {bad:?}"
            );
        }
        // Valid shape but forged checksum.
        let body = serde_json::json!({"chk": "deadbeef", "q": 3, "s": "sess_a"});
        let forged = Cursor::new(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(canon::canonical_string(&body).as_bytes()),
        );
        assert!(SubscribeCursor::decode(&forged).is_err());
        // Unknown extra field.
        let mut value = serde_json::json!({
            "chk": cursor_checksum("sess_a", 3),
            "q": 3,
            "s": "sess_a",
        });
        value["extra"] = serde_json::Value::Bool(true);
        let smuggled = Cursor::new(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(canon::canonical_string(&value).as_bytes()),
        );
        assert!(SubscribeCursor::decode(&smuggled).is_err());
    }

    #[test]
    fn hub_delivers_in_seq_order_despite_out_of_order_publish() {
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        // Publish 3 then 1 then 2: 3 parks, 1 delivers and flushes.
        hub.publish(&[envelope("sess_a", 3, false)]);
        hub.publish(&[envelope("sess_a", 1, false)]);
        hub.publish(&[envelope("sess_a", 2, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(seen, vec![1, 2, 3], "delivery must be seq-ordered");
    }

    #[test]
    fn hub_dedups_duplicate_publication_by_seq_and_event_id() {
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        let events = vec![envelope("sess_a", 1, false), envelope("sess_a", 2, false)];
        hub.publish(&events);
        hub.publish(&events); // exact duplicate publication (idempotent replay)
        hub.publish(&[envelope("sess_a", 1, false)]); // single stale duplicate
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push((e.seq.value(), e.event_id.to_string())),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(
            seen.len(),
            2,
            "duplicates by seq/eventId are dropped: {seen:?}"
        );
        assert_eq!(seen[0].0, 1);
        assert_eq!(seen[1].0, 2);
    }

    #[test]
    fn hub_resumes_mid_stream_after_restart_baseline() {
        // A fresh hub (process restart) seeing seq 7 first must accept it as
        // the baseline instead of parking it forever.
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 6, Some(6));
        hub.release_hold(sub.subscriber_id(), 6);
        hub.publish(&[envelope("sess_a", 8, false)]);
        hub.publish(&[envelope("sess_a", 7, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(seen, vec![7, 8]);
    }

    #[test]
    fn key_event_overflow_detaches_with_explicit_signal() {
        // capacity 3 → 2 event slots + 1 reserved signal slot.
        let hub = EventHub::new(limits(3, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        hub.publish(&[envelope("sess_a", 1, false), envelope("sess_a", 2, false)]);
        // Queue now full for events: a KEY event must not be silently
        // dropped — the subscription detaches with the rebuild directive.
        hub.publish(&[envelope("sess_a", 3, false)]);
        let mut events = Vec::new();
        let mut signal = None;
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => events.push(e.seq.value()),
                SubscriptionFrame::SnapshotRequired { reason } => signal = Some(reason),
            }
        }
        assert_eq!(events, vec![1, 2]);
        assert_eq!(signal, Some(DetachReason::SlowConsumer));
        // The detached subscriber receives nothing further.
        hub.publish(&[envelope("sess_a", 4, false)]);
        assert!(sub.mailbox().try_recv().is_none());
        assert_eq!(
            hub.subscriber_stats(sub.subscriber_id()).unwrap().detached,
            Some(DetachReason::SlowConsumer)
        );
    }

    #[test]
    fn delta_overflow_drops_the_delta_but_keeps_key_events() {
        // capacity 3 → events fill 2 slots; a delta overflows and is
        // dropped (bounded, counted); the following key event still
        // detaches rather than being lost — deltas never displace keys.
        let hub = EventHub::new(limits(3, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        hub.publish(&[envelope("sess_a", 1, true), envelope("sess_a", 2, true)]);
        hub.publish(&[envelope("sess_a", 3, true)]); // delta overflow → dropped
        let stats = hub.subscriber_stats(sub.subscriber_id()).unwrap();
        assert_eq!(stats.dropped_deltas, 1);
        assert_eq!(stats.detached, None, "delta loss does not detach");
        // A subsequent key event with a full queue still detaches loudly.
        hub.publish(&[envelope("sess_a", 4, false)]);
        let mut kinds = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            kinds.push(match frame {
                SubscriptionFrame::Event(_) => "event",
                SubscriptionFrame::SnapshotRequired { .. } => "signal",
            });
        }
        assert_eq!(kinds, vec!["event", "event", "signal"]);
    }

    #[test]
    fn reorder_bound_overflow_breaks_the_stream_loudly() {
        let hub = EventHub::new(limits(8, 2));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        hub.publish(&[envelope("sess_a", 1, false)]);
        // Park 3 events beyond seq 1 with bound 2 → broken stream.
        hub.publish(&[
            envelope("sess_a", 3, false),
            envelope("sess_a", 4, false),
            envelope("sess_a", 5, false),
        ]);
        let mut saw_signal = false;
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => assert_eq!(e.seq.value(), 1),
                SubscriptionFrame::SnapshotRequired { reason } => {
                    assert_eq!(reason, DetachReason::PublicationGap);
                    saw_signal = true;
                }
            }
        }
        assert!(saw_signal, "publication gap must detach explicitly");
    }

    #[test]
    fn hold_buffers_events_and_releases_at_the_cut() {
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        // Registration is in the HOLD state: nothing is delivered yet.
        hub.publish(&[envelope("sess_a", 1, false), envelope("sess_a", 2, false)]);
        assert!(sub.mailbox().try_recv().is_none(), "hold must buffer");
        // The durable cut pins the boundary at 2: the overlap (<= 2) is
        // dropped (it is in the snapshot), the tail (> 2) is delivered.
        hub.release_hold(sub.subscriber_id(), 2);
        hub.publish(&[envelope("sess_a", 3, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(seen, vec![3], "overlap deduped server-side, tail delivered");
    }

    // ── REVIEW-R1 F01 regressions: the hold-join flush must not be
    //    suppressed by the dedup watermark the hold itself advanced. ────────

    #[test]
    fn held_event_above_the_cut_flushes_after_the_hold_advanced_the_watermark() {
        // The production join window (EventService::subscribe): register
        // (hold) → durable page read → release_hold(cut), with NO
        // synchronization between the read result and the release. This
        // test drives exactly that interleaving deterministically (the
        // structured-scheduling counterpart of the multi_thread race test
        // in event_subscription.rs; zero sleeps): events 1..3 are published
        // while the hold is open — event 3 models a commit that landed
        // after the read's snapshot — and the cut (2) is pinned AFTER the
        // hold already holds seq 3. Before the repair the hold had advanced
        // `last_enqueued_seq` to 3, so the flush of event 3 hit the
        // duplicate check in `enqueue_for` and was silently lost (REVIEW-R1
        // probe A delivered [4] instead of [3,4], with no detach, no
        // snapshot_required and no loss counter).
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.publish(&[envelope("sess_a", 1, false)]);
        hub.publish(&[envelope("sess_a", 2, false)]);
        hub.publish(&[envelope("sess_a", 3, false)]); // buffered in the hold, seq > future cut
        hub.release_hold(sub.subscriber_id(), 2); // the durable read pinned cut = 2
        hub.publish(&[envelope("sess_a", 4, false)]); // live after the join
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(
            seen,
            vec![3, 4],
            "held event above the cut must flush at release_hold, in order"
        );
        assert_eq!(
            hub.subscriber_stats(sub.subscriber_id()).unwrap(),
            SubscriberStats {
                stream_id: "sess_a".to_string(),
                last_enqueued_seq: Seq::new(4),
                dropped_deltas: 0,
                detached: None,
            },
            "the flushed tail must re-advance the watermark (no silent suppression)"
        );
    }

    #[test]
    fn held_delta_above_the_cut_is_delivered_not_counted_lost() {
        // REVIEW-R1 probe A2: the same window with a lossy delta above the
        // cut. The join boundary must not bypass the loss accounting: the
        // delta is DELIVERED by the flush (dropped_deltas stays 0) — a
        // bookkeeping slip is not the bounded, counted overflow loss.
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.publish(&[envelope("sess_a", 1, false)]);
        hub.publish(&[envelope("sess_a", 2, true)]); // delta buffered in the hold
        hub.release_hold(sub.subscriber_id(), 1); // cut = 1; held delta 2 is above it
        hub.publish(&[envelope("sess_a", 3, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(seen, vec![2, 3], "held delta above the cut must flush too");
        let stats = hub.subscriber_stats(sub.subscriber_id()).unwrap();
        assert_eq!(
            stats.dropped_deltas, 0,
            "a flushed hold delta is a delivery, not an overflow loss"
        );
        assert_eq!(stats.detached, None);
    }

    // ── REVIEW-R1 F02 regressions: the restart baseline must drain the
    //    parked publication that lands exactly on the new next_seq. ─────────

    #[test]
    fn restart_baseline_drains_the_parked_event_at_the_new_next_seq() {
        // REVIEW-R1 probe B: post-restart, a publication arriving before
        // any subscriber registers parks behind the never-published
        // historical seqs (fresh hub next_seq = 1). The baseline alignment
        // (register with known_head = 2) sets next_seq onto the parked seq
        // — before the repair nothing ever drained it (its drain depends on
        // a SMALLER seq arriving, which the baseline rules out) and live
        // delivery stalled forever, silently, for every later subscriber.
        let hub = EventHub::new(limits(8, 8));
        hub.publish(&[envelope("sess_a", 3, false)]); // parks (next_seq = 1)
        let sub = hub.register("sess_a", 0, Some(2)); // baseline alignment: next_seq = 3
                                                      // The durable page read follows the registration and sees the
                                                      // committed event 3, so the cut pins 3 (the subscriber receives 3
                                                      // from the cut, not live).
        hub.release_hold(sub.subscriber_id(), 3);
        hub.publish(&[envelope("sess_a", 4, false)]);
        hub.publish(&[envelope("sess_a", 5, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(
            seen,
            vec![4, 5],
            "live delivery must not stall behind the aligned baseline"
        );
    }

    #[test]
    fn restart_baseline_drains_a_parked_chain_above_the_head() {
        // Same alignment with a contiguous parked chain: the whole chain
        // from the new baseline drains (each event was published, hence
        // committed before the registration; the durable cuts taken from
        // now on cover them), and live delivery continues seamlessly.
        let hub = EventHub::new(limits(8, 8));
        hub.publish(&[envelope("sess_a", 3, false)]);
        hub.publish(&[envelope("sess_a", 4, false)]); // parked chain {3, 4}
        let sub = hub.register("sess_a", 0, Some(2)); // next_seq 1 → 3; chain drains
        hub.release_hold(sub.subscriber_id(), 4); // cut covers the drained 3 and 4
        hub.publish(&[envelope("sess_a", 5, false)]);
        let mut seen = Vec::new();
        while let Some(frame) = sub.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => seen.push(e.seq.value()),
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert_eq!(
            seen,
            vec![5],
            "delivery continues from the drained baseline"
        );
    }

    #[test]
    fn drop_guard_unregisters_and_prunes_on_publish() {
        let hub = EventHub::new(limits(8, 8));
        let sub = hub.register("sess_a", 0, None);
        hub.release_hold(sub.subscriber_id(), 0);
        let mailbox = sub.mailbox().clone();
        drop(sub);
        assert!(
            mailbox.try_recv().is_none(),
            "closed mailbox drains to None"
        );
        // Publishing to a stream whose only subscriber is gone must not
        // panic or grow the hub (closed-consumer pruning).
        hub.publish(&[envelope("sess_a", 1, false)]);
        assert_eq!(hub.subscriber_stats(0), None, "pruned after close");
    }

    #[test]
    fn control_frames_carry_the_control_marker_and_string_seqs() {
        let cut = EventCut {
            stream_id: "sess_a".to_string(),
            mode: "snapshot",
            from_seq: Seq::new(0),
            snapshot_seq: Seq::new((1u64 << 53) + 1),
            events: Vec::new(),
            next_cursor: None,
        };
        let json = control_subscribed_json(&cut);
        let value: serde_json::Value = serde_json::from_str(&json).expect("canonical json");
        assert_eq!(value["frameKind"], "control");
        assert_eq!(value["type"], "subscribed");
        assert_eq!(value["snapshotSeq"], serde_json::json!("9007199254740993"));
        // Control frames never collide with the envelope field set.
        assert!(value.get("eventId").is_none());

        let required =
            control_snapshot_required_json("sess_a", Some(Seq::new(7)), "events_truncated");
        let value: serde_json::Value = serde_json::from_str(&required).expect("canonical json");
        assert_eq!(value["frameKind"], "control");
        assert_eq!(value["type"], "snapshot_required");
        assert_eq!(value["floorSeq"], serde_json::json!("7"));
        assert_eq!(value["reason"], "events_truncated");
    }

    #[test]
    fn event_limits_validation_is_loud() {
        assert!(EventLimits::default().validate().is_ok());
        assert!(limits(1, 8).validate().is_err(), "capacity < 2 rejected");
        assert!(limits(8, 0).validate().is_err(), "pending bound 0 rejected");
        let bad = EventLimits {
            page_limit_default: 501,
            ..EventLimits::default()
        };
        assert!(bad.validate().is_err(), "default > max rejected");
    }
}
