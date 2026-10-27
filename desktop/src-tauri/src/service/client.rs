//! R09-T02 服务身份/健康 HTTP 校验（C14/C24）。
//!
//! 与 Electron `main.cjs:1310-1334` 同语义：
//! - `GET {baseUrl}/lingxi/v1/server/identity`，`Authorization: Bearer <token>`；
//! - 每请求 5s 超时，仅连接拒绝/重置重试 20×100ms（READY 与 accept 循环之间的调度片）；
//! - 要求 `serverNodeKind === 'lingxi-service'`、transport 匹配、serverId/studioId 非空；
//! - 健康面 `GET /lingxi/v1/health`（公开）核对 wire/epoch/serverKind/status。
//!
//! 注意：`serverVersion()` 仍是 0.0.0 占位（R02 已知缺口），不参与身份门；
//! 协议/数据版本由 `instance.json` 与 health 的数值字段把关。

use std::time::Duration;

use serde_json::Value;

use super::error::{codes, ServiceResult, StartError};
use super::manifest::{ProtocolIdentity, SERVER_KIND};

pub const IDENTITY_PATH: &str = "/lingxi/v1/server/identity";
pub const HEALTH_PATH: &str = "/lingxi/v1/health";
pub const IDENTITY_TIMEOUT: Duration = Duration::from_secs(5);
pub const IDENTITY_RETRIES: u32 = 20;
pub const IDENTITY_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub server_node_kind: String,
    pub server_node_transport: String,
    pub server_id: String,
    pub studio_id: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub status: String,
    pub server_kind: String,
    pub wire_protocol_min: u64,
    pub wire_protocol_max: u64,
    pub data_epoch: u64,
    pub raw: Value,
}

fn http_client(timeout: Duration) -> ServiceResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| {
            StartError::new(
                codes::SERVICE_IDENTITY_FAILED,
                format!("cannot build loopback HTTP client: {error}"),
            )
        })
}

fn identity_error(message: impl Into<String>, detail: Value) -> StartError {
    StartError::new(codes::SERVICE_IDENTITY_FAILED, message).with_detail(detail)
}

fn expect_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

pub async fn verify_identity(
    base_url: &str,
    token: &str,
    transport: &str,
) -> ServiceResult<Identity> {
    let client = http_client(IDENTITY_TIMEOUT)?;
    let url = format!("{base_url}{IDENTITY_PATH}");
    let mut last_error: Option<reqwest::Error> = None;
    let mut response = None;
    for attempt in 0..=IDENTITY_RETRIES {
        let result = client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await;
        match result {
            Ok(value) => {
                response = Some(value);
                break;
            }
            Err(error) => {
                let retryable = error.is_connect() || error.is_timeout();
                if !retryable || attempt == IDENTITY_RETRIES {
                    last_error = Some(error);
                    break;
                }
                tokio::time::sleep(IDENTITY_RETRY_DELAY).await;
            }
        }
    }
    let response = match response {
        Some(response) => response,
        None => {
            let detail = last_error
                .as_ref()
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no response".to_string());
            return Err(StartError::new(
                codes::SERVICE_IDENTITY_FAILED,
                format!("Rust desktop identity request never reached the service: {detail}"),
            )
            .with_detail(serde_json::json!({"url": url, "transport": transport})));
        }
    };
    if !response.status().is_success() {
        return Err(identity_error(
            format!(
                "Rust desktop identity returned HTTP {}",
                response.status().as_u16()
            ),
            serde_json::json!({"status": response.status().as_u16(), "url": url}),
        ));
    }
    let identity: Value = response.json().await.map_err(|error| {
        identity_error(
            format!("Rust desktop identity response is not JSON: {error}"),
            serde_json::json!({"url": url}),
        )
    })?;
    let server_node_kind = expect_string(&identity, "serverNodeKind").ok_or_else(|| {
        identity_error(
            "Rust desktop identity has no serverNodeKind",
            serde_json::json!({"url": url}),
        )
    })?;
    let server_node_transport =
        expect_string(&identity, "serverNodeTransport").ok_or_else(|| {
            identity_error(
                "Rust desktop identity has no serverNodeTransport",
                serde_json::json!({"url": url}),
            )
        })?;
    let server_id = expect_string(&identity, "serverId").ok_or_else(|| {
        identity_error(
            "Rust desktop identity has no serverId",
            serde_json::json!({"url": url}),
        )
    })?;
    let studio_id = expect_string(&identity, "studioId").ok_or_else(|| {
        identity_error(
            "Rust desktop identity has no studioId",
            serde_json::json!({"url": url}),
        )
    })?;
    if server_node_kind != SERVER_KIND {
        return Err(StartError::mismatch(
            codes::RUNTIME_IDENTITY_MISMATCH,
            "service identity is not a lingxi-service node",
            SERVER_KIND,
            server_node_kind.clone(),
        )
        .with_detail(serde_json::json!({"url": url})));
    }
    if server_node_transport != transport {
        return Err(StartError::mismatch(
            codes::RUNTIME_IDENTITY_MISMATCH,
            "service identity transport does not match the resolved connection",
            transport.to_string(),
            server_node_transport.clone(),
        ));
    }
    Ok(Identity {
        server_node_kind,
        server_node_transport,
        server_id,
        studio_id,
        raw: identity,
    })
}

pub async fn verify_health(base_url: &str) -> ServiceResult<Health> {
    let client = http_client(IDENTITY_TIMEOUT)?;
    let url = format!("{base_url}{HEALTH_PATH}");
    let response = client.get(&url).send().await.map_err(|error| {
        identity_error(
            format!("Rust desktop health request failed: {error}"),
            serde_json::json!({"url": url}),
        )
    })?;
    if !response.status().is_success() {
        return Err(identity_error(
            format!(
                "Rust desktop health returned HTTP {}",
                response.status().as_u16()
            ),
            serde_json::json!({"status": response.status().as_u16()}),
        ));
    }
    let health: Value = response.json().await.map_err(|error| {
        identity_error(
            format!("Rust desktop health response is not JSON: {error}"),
            serde_json::json!({"url": url}),
        )
    })?;
    let status = expect_string(&health, "status").unwrap_or_default();
    let server_kind = expect_string(&health, "serverKind").unwrap_or_default();
    let wire_protocol_min = health.get("wireProtocolMin").and_then(Value::as_u64);
    let wire_protocol_max = health.get("wireProtocolMax").and_then(Value::as_u64);
    let data_epoch = health.get("dataEpoch").and_then(Value::as_u64);
    let host = ProtocolIdentity::host();
    let wire_ok = wire_protocol_min == Some(host.wire_protocol_min as u64)
        && wire_protocol_max == Some(host.wire_protocol_max as u64)
        && data_epoch == Some(host.data_epoch as u64);
    if status != "ok" || server_kind != SERVER_KIND || !wire_ok {
        return Err(StartError::mismatch(
            codes::RUNTIME_IDENTITY_MISMATCH,
            "service health does not match the host protocol/identity contract",
            format!(
                "status=ok serverKind={SERVER_KIND} wire {}..={} epoch {}",
                host.wire_protocol_min, host.wire_protocol_max, host.data_epoch
            ),
            format!(
                "status={status} serverKind={server_kind} wire {}..={} epoch {}",
                wire_protocol_min
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
                wire_protocol_max
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
                data_epoch
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".into()),
            ),
        )
        .with_detail(serde_json::json!({"url": url})));
    }
    Ok(Health {
        status,
        server_kind,
        wire_protocol_min: wire_protocol_min.unwrap_or(0),
        wire_protocol_max: wire_protocol_max.unwrap_or(0),
        data_epoch: data_epoch.unwrap_or(0),
        raw: health,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_gate_compares_against_lingxi_protocol_constants() {
        let host = ProtocolIdentity::host();
        assert_eq!(host.wire_protocol_min, 1);
        assert_eq!(host.wire_protocol_max, 1);
        assert_eq!(host.data_epoch, 2);
    }

    #[test]
    fn identity_requires_all_fields() {
        // 纯字段断言：真实 HTTP 由集成测试 `service_roundtrip` 覆盖。
        let value = serde_json::json!({"serverNodeKind": "lingxi-service"});
        assert!(expect_string(&value, "serverId").is_none());
        assert!(expect_string(&value, "studioId").is_none());
    }
}
