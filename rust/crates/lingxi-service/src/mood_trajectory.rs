//! 情绪专用扩展事件。正文和推理的既有事件保持原语义，不混入情绪内容。
//! 扩展载荷遵循 wire 的未知事件保留约定；本服务和新版展示明确支持它。
use crate::streaming_norm::NormEvent;
use lingxi_protocol::{
    AssistantPhase, AssistantSegmentDeltaPayload, AssistantSegmentEndPayload,
    AssistantSegmentStartPayload, EventPayload, KnownEventPayload, SegmentKind,
    UnknownEventPayload,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "stage",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum MoodPayload {
    Start { segment_id: String },
    Delta { segment_id: String, delta: String },
    End { segment_id: String },
}

pub(crate) fn extension(event: &NormEvent) -> Option<EventPayload> {
    let payload = match event {
        NormEvent::SegmentStart {
            segment_id,
            kind: SegmentKind::Mood,
            ..
        } => MoodPayload::Start {
            segment_id: segment_id.clone(),
        },
        NormEvent::SegmentDelta {
            segment_id, delta, ..
        } if segment_id.contains(":mood:") => MoodPayload::Delta {
            segment_id: segment_id.clone(),
            delta: delta.clone(),
        },
        NormEvent::SegmentEnd { segment_id, .. } if segment_id.contains(":mood:") => {
            MoodPayload::End {
                segment_id: segment_id.clone(),
            }
        }
        _ => return None,
    };
    let mut raw = serde_json::to_value(payload)
        .expect("情绪事件可序列化")
        .as_object()
        .unwrap()
        .clone();
    raw.insert("type".into(), "assistant_mood".into());
    Some(EventPayload::Unknown(UnknownEventPayload {
        event_type: "assistant_mood".into(),
        raw,
    }))
}

/// 仅投影自己已定义且完整合法的扩展，其余未知事件仍留在原事件面。
pub(crate) fn projection(payload: &UnknownEventPayload) -> Option<KnownEventPayload> {
    if payload.event_type != "assistant_mood"
        || payload.raw.get("type")?.as_str()? != "assistant_mood"
    {
        return None;
    }
    let mut raw = payload.raw.clone();
    raw.remove("type");
    let payload: MoodPayload = serde_json::from_value(raw.into()).ok()?;
    let segment_id = match &payload {
        MoodPayload::Start { segment_id }
        | MoodPayload::Delta { segment_id, .. }
        | MoodPayload::End { segment_id } => segment_id,
    };
    let parts: Vec<_> = segment_id.split(':').collect();
    if !matches!(parts.len(), 4 | 5)
        || parts[0] != "assistant"
        || parts[2] != "mood"
        || parts[1].parse::<u32>().ok().is_none_or(|n| n == 0)
        || parts[3].parse::<u32>().ok().is_none_or(|n| n == 0)
        || (parts.len() == 5
            && (parts[4].len() != 64 || !parts[4].bytes().all(|b| b.is_ascii_hexdigit())))
    {
        return None;
    }
    let phase = AssistantPhase::Unresolved;
    Some(match payload {
        MoodPayload::Start { segment_id } => {
            KnownEventPayload::AssistantSegmentStart(AssistantSegmentStartPayload {
                segment_id,
                kind: SegmentKind::Mood,
                semantic_phase: phase,
            })
        }
        MoodPayload::Delta { segment_id, delta } => {
            KnownEventPayload::AssistantSegmentDelta(AssistantSegmentDeltaPayload {
                segment_id,
                delta,
                semantic_phase: phase,
            })
        }
        MoodPayload::End { segment_id } => {
            KnownEventPayload::AssistantSegmentEnd(AssistantSegmentEndPayload {
                segment_id,
                semantic_phase: phase,
            })
        }
    })
}
