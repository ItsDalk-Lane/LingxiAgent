//! 只从真实运行事件取可见草稿；推理、半截时间标记和未追平的旧页不外发。
use lingxi_adapters::storage::RunDatabase;
use lingxi_protocol::{AssistantPhase, EventPayload, KnownEventPayload, Seq};

const MAX_DRAFT_BYTES: usize = 100_000;
#[derive(Default)]
pub(crate) struct VisibleDraft {
    cursor: u64,
    text: String,
    caught_up: bool,
}
impl VisibleDraft {
    pub async fn refresh(
        &mut self,
        db: &RunDatabase,
        session: &str,
        run: &str,
    ) -> Result<(), String> {
        let slice = db
            .stream_resume_slice(session, Seq::new(self.cursor), 256)
            .await
            .map_err(|e| e.to_string())?;
        if self.cursor > 0
            && slice
                .floor
                .is_some_and(|floor| floor.value() > self.cursor.saturating_add(1))
        {
            return Err("Bridge draft event history was truncated".into());
        }
        for event in slice.events {
            if self.cursor > 0 && event.seq.value() != self.cursor + 1 {
                return Err("Bridge draft event sequence has a gap".into());
            }
            self.cursor = event.seq.value();
            if event.run_id.as_ref().map(|id| id.as_str()) != Some(run) {
                continue;
            }
            match event.payload {
                EventPayload::Known(KnownEventPayload::ModelCallStarted(_)) => self.text.clear(),
                EventPayload::Known(KnownEventPayload::ModelCallDelta(delta))
                    if delta.phase != AssistantPhase::Reasoning =>
                {
                    if self.text.len().saturating_add(delta.delta.len()) > MAX_DRAFT_BYTES {
                        return Err("Bridge draft exceeds its byte limit".into());
                    }
                    self.text.push_str(&delta.delta);
                }
                _ => {}
            }
        }
        self.caught_up = slice.head.is_none_or(|head| self.cursor == head.value());
        Ok(())
    }
    pub fn visible(&self) -> Option<String> {
        if !self.caught_up {
            return None;
        }
        let mut end = self.text.len();
        if let Some(open) = self.text.rfind('<') {
            if !self.text[open..].contains('>') {
                end = end.min(open);
            }
        }
        if let Some(open) = self.text.rfind("<t>") {
            if !self.text[open + 3..].contains("</t>") {
                end = end.min(open);
            }
        }
        let text = crate::history::history_content::sanitize_bridge_visible_text(&self.text[..end]);
        (!text.is_empty()).then_some(text)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_withholds_incomplete_time_marker_and_never_publishes_backlog() {
        let mut draft = VisibleDraft {
            cursor: 0,
            text: "<t> 10-16 09:30 <".into(),
            caught_up: true,
        };
        assert_eq!(draft.visible(), None);
        draft.text.push_str("/t>真实文本");
        assert_eq!(draft.visible(), Some("真实文本".into()));
        draft.text.push_str("<t> 10-16");
        assert_eq!(draft.visible(), Some("真实文本".into()));
        draft.caught_up = false;
        assert_eq!(draft.visible(), None);
    }
}
