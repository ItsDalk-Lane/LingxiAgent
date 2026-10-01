
// ── STAGE-REVIEWER-R04-RR1: independent F04 counterexample (appended,
// test-only, worktree-local — never part of any product or committed
// candidate). Uses the private TranscriptCore directly so the same source
// compiles against BOTH trees and is a runtime red on 773d5a696. ──────────
#[cfg(test)]
mod zz_stage_r1_f04 {
    use super::*;

    fn core() -> TranscriptCore {
        TranscriptCore {
            chunks: VecDeque::new(),
            ring_bytes: 0,
            ring_cap: 4096,
            next_seq: 1,
            cursor_seq: 0,
            dropped_undelivered_bytes: 0,
            total_bytes: 0,
            spill: None,
        }
    }

    #[test]
    fn zz_stage_r1_f04_mixed_chunk_prefix_is_consumed_exactly_once() {
        let mut t = core();
        // One chunk holding 'a' (0x61) plus the FIRST TWO bytes of 日.
        t.append(&[0x61, 0xE6, 0x97]);
        let d1 = t.deliver_since_cursor(false);
        assert_eq!(d1.text, "a", "the decodable prefix is delivered");
        assert_eq!(
            d1.dropped_undelivered_bytes, 0,
            "STAGE-R1-F04: bytes held back awaiting completion are NOT dropped"
        );

        // An idle poll with nothing new must return EMPTY — not a
        // re-delivery of the already-consumed prefix 'a'.
        let d2 = t.deliver_since_cursor(false);
        assert_eq!(
            d2.text, "",
            "STAGE-R1-F04: idle poll re-delivered the consumed prefix"
        );

        // The completion byte arrives; only the held-back character may be
        // delivered now.
        t.append(&[0xA5]);
        let d3 = t.deliver_since_cursor(false);
        assert_eq!(d3.text, "日");

        let joined = format!("{}{}{}", d1.text, d2.text, d3.text);
        assert_eq!(
            joined, "a日",
            "STAGE-R1-F04: the stream must be delivered exactly once, not duplicated"
        );
    }
}
