//! 后台通知的统一回执；领取、显示与确认是不同事实。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AutomationNotificationStatus {
    Queued,
    Started,
    Confirmed,
    Unknown,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationNotificationReceipt {
    pub delivery_id: String,
    pub trigger_id: Option<String>,
    pub run_id: Option<String>,
    pub channel: String,
    pub recipient: String,
    pub payload_json: String,
    pub status: AutomationNotificationStatus,
    pub platform_receipt: Option<Value>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationNotificationClaim {
    pub reserved: bool,
    pub receipt: AutomationNotificationReceipt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationDisplayReceipt {
    pub delivery_id: String,
    pub channel: String,
    pub displayed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationNotificationCommand {
    Claim {
        delivery_id: String,
    },
    Ack {
        delivery_id: String,
        platform_receipt: AutomationDisplayReceipt,
    },
    Failed {
        delivery_id: String,
        error: String,
    },
    Unknown {
        delivery_id: String,
        error: String,
    },
}

impl AutomationNotificationCommand {
    pub fn delivery_id(&self) -> &str {
        match self {
            Self::Claim { delivery_id }
            | Self::Ack { delivery_id, .. }
            | Self::Failed { delivery_id, .. }
            | Self::Unknown { delivery_id, .. } => delivery_id,
        }
    }

    /// 格式正确不代表已经展示；确认命令还必须绑定这次实际桌面回执。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.delivery_id().trim().is_empty() {
            return Err("deliveryId must be non-empty");
        }
        match self {
            Self::Ack {
                delivery_id,
                platform_receipt,
            } if platform_receipt.delivery_id != *delivery_id
                || platform_receipt.channel != "desktop"
                || !platform_receipt.displayed =>
            {
                Err("notification display receipt mismatch")
            }
            Self::Failed { error, .. } | Self::Unknown { error, .. } if error.trim().is_empty() => {
                Err("notification failure error must be non-empty")
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn commands_use_one_closed_camel_case_shape() {
        let fixtures = [
            json!({"action":"claim","deliveryId":"d1"}),
            json!({"action":"ack","deliveryId":"d1","platformReceipt":{"deliveryId":"d1","channel":"desktop","displayed":true}}),
            json!({"action":"failed","deliveryId":"d1","error":"桌面系统未确认显示"}),
            json!({"action":"unknown","deliveryId":"d1","error":"展示结果未确认"}),
        ];
        for value in fixtures {
            let command: AutomationNotificationCommand =
                serde_json::from_value(value.clone()).unwrap();
            command.validate().unwrap();
            assert_eq!(serde_json::to_value(command).unwrap(), value);
        }
        for value in [
            json!({"action":"claim","deliveryId":"d1","displayed":true}),
            json!({"action":"ack","deliveryId":"d1","platformReceipt":{"deliveryId":"d1","channel":"desktop","displayed":true,"trusted":true}}),
            json!({"action":"failed","deliveryId":"d1","error":"failed","reserved":true}),
            json!({"action":"ack","delivery_id":"d1","platformReceipt":{"deliveryId":"d1","channel":"desktop","displayed":true}}),
            json!({"action":"confirm","deliveryId":"d1"}),
        ] {
            assert!(serde_json::from_value::<AutomationNotificationCommand>(value).is_err());
        }
    }

    #[test]
    fn confirmation_requires_matching_positive_desktop_display_receipt() {
        for (delivery, channel, displayed) in [
            ("other", "desktop", true),
            ("d1", "bridge_owner", true),
            ("d1", "desktop", false),
        ] {
            let command = AutomationNotificationCommand::Ack {
                delivery_id: "d1".into(),
                platform_receipt: AutomationDisplayReceipt {
                    delivery_id: delivery.into(),
                    channel: channel.into(),
                    displayed,
                },
            };
            assert!(command.validate().is_err());
        }
        assert!(AutomationNotificationCommand::Claim {
            delivery_id: " ".into()
        }
        .validate()
        .is_err());
        assert!(AutomationNotificationCommand::Failed {
            delivery_id: "d1".into(),
            error: " ".into()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn all_durable_states_round_trip_without_claiming_delivery() {
        for (status, wire) in [
            (AutomationNotificationStatus::Queued, "queued"),
            (AutomationNotificationStatus::Started, "started"),
            (AutomationNotificationStatus::Confirmed, "confirmed"),
            (AutomationNotificationStatus::Unknown, "unknown"),
            (AutomationNotificationStatus::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<AutomationNotificationStatus>(json!(wire)).unwrap(),
                status
            );
        }
        assert!(serde_json::from_value::<AutomationNotificationStatus>(json!("sent")).is_err());
        let value = json!({"deliveryId":"d1","triggerId":null,"runId":"r1","channel":"desktop","recipient":"user_local","payloadJson":"{}","status":"started","platformReceipt":null,"error":null,"createdAt":1800000000000i64,"updatedAt":1800000000001i64});
        let receipt: AutomationNotificationReceipt = serde_json::from_value(value.clone()).unwrap();
        let claim = AutomationNotificationClaim {
            reserved: true,
            receipt,
        };
        assert_eq!(claim.receipt.status, AutomationNotificationStatus::Started);
        assert!(
            claim.receipt.platform_receipt.is_none(),
            "领取成功不能补出显示回执"
        );
        let mut extra = value;
        extra["displayed"] = json!(true);
        assert!(serde_json::from_value::<AutomationNotificationReceipt>(extra).is_err());
        assert!(serde_json::from_value::<AutomationNotificationClaim>(
            json!({"reserved":true,"receipt":claim.receipt,"displayed":true})
        )
        .is_err());
    }
}
