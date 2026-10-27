//! 当前桌面浏览器宿主：只接受本机所有者接线，恢复命令必须等真实宿主回执。
use crate::{ws, EndpointError, Principal, ServiceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tokio::sync::{mpsc, oneshot};

struct ResumeCommand {
    session: String,
    reply: oneshot::Sender<Result<Value, String>>,
}
struct PendingResume {
    id: u64,
    reply: oneshot::Sender<Result<Value, String>>,
    deadline: tokio::time::Instant,
}
struct Host {
    generation: u64,
    sender: mpsc::Sender<ResumeCommand>,
}
#[derive(Default)]
pub struct BrowserHost {
    host: Mutex<Option<Host>>,
    sequence: AtomicU64,
}
impl BrowserHost {
    pub async fn resume(&self, session: &str) -> Result<Value, String> {
        let sender = self
            .host
            .lock()
            .map_err(|_| "浏览器宿主状态不可读")?
            .as_ref()
            .map(|host| host.sender.clone());
        let Some(sender) = sender else {
            return Ok(json!({"status":"skipped","reason":"no_browser_host"}));
        };
        let (reply, receipt) = oneshot::channel();
        sender
            .try_send(ResumeCommand {
                session: session.into(),
                reply,
            })
            .map_err(|_| "浏览器宿主繁忙或已断开")?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(6), receipt)
            .await
            .map_err(|_| "浏览器恢复回执超时")?
            .map_err(|_| "浏览器宿主已断开")??;
        let found = result
            .get("found")
            .and_then(Value::as_bool)
            .ok_or("浏览器恢复回执格式错误")?;
        let url = result
            .get("url")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_str()
                    .filter(|s| s.len() <= 8192)
                    .ok_or("浏览器地址回执格式错误")
            })
            .transpose()?;
        Ok(if found {
            json!({"status":"resumed","found":true,"url":url})
        } else {
            json!({"status":"attempted","found":false,"reason":"no_session_browser"})
        })
    }
}

pub async fn route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    upgrade: Result<ws::WsUpgrade, ws::WsUpgradeRejection>,
) -> Response {
    if !principal.is_local_owner() {
        return EndpointError::new(
            axum::http::StatusCode::FORBIDDEN,
            lingxi_protocol::ErrorCode::Forbidden,
            "浏览器宿主仅限本机所有者",
        )
        .into_response();
    }
    let upgrade = match upgrade {
        Ok(value) => value,
        Err(error) => {
            return EndpointError::new(
                axum::http::StatusCode::BAD_REQUEST,
                lingxi_protocol::ErrorCode::InvalidMessage,
                error.reason,
            )
            .into_response()
        }
    };
    let Some(slot) = state.ws_conns_arc().acquire() else {
        return EndpointError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            lingxi_protocol::ErrorCode::BudgetExceeded,
            "浏览器连接已达上限",
        )
        .into_response();
    };
    let (sender, commands) = mpsc::channel(16);
    let generation = state.browser_host.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut host = state
            .browser_host
            .host
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if host.is_some() {
            return EndpointError::new(
                axum::http::StatusCode::CONFLICT,
                lingxi_protocol::ErrorCode::Conflict,
                "浏览器宿主已连接",
            )
            .into_response();
        }
        *host = Some(Host { generation, sender });
    }
    let response = ws::switching_protocols_response(&upgrade.sec_websocket_key);
    tokio::spawn(async move {
        if let Ok(io) = upgrade.on_upgrade.await {
            run(
                hyper_util::rt::tokio::TokioIo::new(io),
                &state,
                principal,
                transport.0,
                commands,
                slot,
            )
            .await;
        }
        let mut host = state
            .browser_host
            .host
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if host
            .as_ref()
            .is_some_and(|host| host.generation == generation)
        {
            *host = None;
        }
    });
    response
}

async fn run<IO>(
    io: IO,
    state: &ServiceState,
    principal: Principal,
    secure: bool,
    mut commands: mpsc::Receiver<ResumeCommand>,
    _slot: crate::limits::WsConnectionGuard,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut shutdown = state.ws_shutdown().subscribe();
    let _guard = crate::shutdown::WsSessionGuard::new(state.ws_shutdown());
    let (reader, mut writer) = tokio::io::split(io);
    let mut reader = ws::ClientFrameReader::new(reader);
    let mut auth_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    auth_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending: Option<PendingResume> = None;
    let mut sequence = 0_u64;
    loop {
        let deadline = pending
            .as_ref()
            .map(|p| p.deadline)
            .unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(3600));
        tokio::select! {
            _ = shutdown.changed() => { let _=ws::write_ws_close(&mut writer,1001,"shutdown").await; break; }
            _ = auth_tick.tick() => { if !crate::ensure_ws_principal_current(&mut writer,state,&principal,secure).await { break; } }
            _ = tokio::time::sleep_until(deadline), if pending.is_some() => {
                if let Some(PendingResume { reply, .. })=pending.take() { let _=reply.send(Err("浏览器恢复回执超时".into())); }
            }
            command = commands.recv(), if pending.is_none() => {
                let Some(command)=command else { break; };
                if command.reply.is_closed() { continue; }
                if !crate::ensure_ws_principal_current(&mut writer,state,&principal,secure).await { break; }
                sequence+=1;
                let message=json!({"type":"browser-cmd","id":sequence,"cmd":"resume","params":{"sessionPath":crate::session_locator::native(&command.session)}});
                if ws::write_ws_text(&mut writer,&serde_json::to_vec(&message).expect("JSON浏览器命令")).await.is_err() { let _=command.reply.send(Err("浏览器宿主发送失败".into())); break; }
                pending=Some(PendingResume { id: sequence, reply: command.reply, deadline: tokio::time::Instant::now()+std::time::Duration::from_secs(5) });
            }
            frame = reader.recv() => {
                match frame {
                    Ok(Some(ws::WsFrame::Ping(data))) => { if ws::write_ws_frame(&mut writer,0xA,&data).await.is_err(){break;} }
                    Ok(Some(ws::WsFrame::Pong)) => {},
                    Ok(Some(ws::WsFrame::Text(bytes))) => {
                        if !crate::ensure_ws_principal_current(&mut writer,state,&principal,secure).await { break; }
                        let Ok(message)=serde_json::from_slice::<Value>(&bytes) else { let _=ws::write_ws_close(&mut writer,1007,"invalid_json").await; break; };
                        if message.get("type").and_then(Value::as_str)!=Some("browser-result") { continue; }
                        if pending.as_ref().is_some_and(|p| message.get("id").and_then(Value::as_u64)==Some(p.id)) {
                            let PendingResume { reply, .. }=pending.take().expect("已匹配浏览器回执");
                            let result=if let Some(error)=message.get("error").and_then(Value::as_str) { Err(error.chars().take(2048).collect()) }
                                       else { message.get("result").cloned().ok_or_else(|| "浏览器恢复缺少回执".into()) };
                            let _=reply.send(result);
                        }
                    }
                    _ => break,
                }
            }
        }
    }
    if let Some(PendingResume { reply, .. }) = pending {
        let _ = reply.send(Err("浏览器宿主连接中断".into()));
    }
    reader.shutdown().await;
}
