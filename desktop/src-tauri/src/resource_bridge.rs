use serde_json::{json, Value};
use std::{net::SocketAddr, path::Path, time::Duration};

/// 凭证留在宿主；只能访问现行 Rust 服务的三个资源监听入口。
pub struct ResourceConnection {
    base: String,
    token: String,
}

impl ResourceConnection {
    pub fn from_home(home: &Path) -> Result<Self, String> {
        if !home.is_absolute() {
            return Err("服务目录必须为绝对路径".into());
        }
        let runtime = home.join("lingxi-service");
        let read = |name: &str| -> Result<Value, String> {
            let bytes =
                std::fs::read(runtime.join(name)).map_err(|_| format!("服务尚未就绪：{name}"))?;
            serde_json::from_slice(&bytes).map_err(|_| format!("服务记录无效：{name}"))
        };
        let record = read("instance.json")?;
        let auth = read("local-token.json")?;
        if record["serverKind"] != "lingxi-service"
            || record["instanceId"].as_str().is_none()
            || record["instanceId"] != auth["instanceId"]
            || auth["kind"] != "local_token"
        {
            return Err("服务身份和本机凭证不匹配".into());
        }
        if record["transport"] != "http" {
            return Err("此桌面入口需要明确配置的本机 HTTP 服务".into());
        }
        let addr: SocketAddr = record["bindAddr"]
            .as_str()
            .ok_or("服务缺少监听地址")?
            .parse()
            .map_err(|_| "服务监听地址无效")?;
        if !addr.ip().is_loopback() || addr.port() == 0 {
            return Err("拒绝非本机服务地址".into());
        }
        let token = auth["token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or("服务缺少本机凭证")?
            .to_owned();
        Ok(Self {
            base: format!("http://{addr}/lingxi/v1/resource-io"),
            token,
        })
    }

    async fn request(
        &self,
        method: reqwest::Method,
        suffix: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let mut request = client
            .request(method, format!("{}{}", self.base, suffix))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "无法连接本机资源服务".to_string())?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|_| "资源服务返回无效数据".to_string())?;
        if !status.is_success() {
            return Err(format!(
                "资源服务拒绝（{}）：{}",
                status.as_u16(),
                value["error"].as_str().unwrap_or("操作失败")
            ));
        }
        Ok(value)
    }

    pub async fn subscribe(&self, path: &str) -> Result<Value, String> {
        self.request(
            reqwest::Method::POST,
            "/subscribe",
            Some(json!({"purpose":"viewer", "resources":[{"kind":"local-file", "path":path}]})),
        )
        .await
    }
    pub async fn events(&self, since: u64) -> Result<Value, String> {
        self.request(
            reqwest::Method::GET,
            &format!("/events?since={since}"),
            None,
        )
        .await
    }
    pub async fn unsubscribe(&self, id: &str) -> Result<Value, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("无效订阅身份".into());
        }
        self.request(
            reqwest::Method::DELETE,
            &format!("/subscriptions/{id}"),
            None,
        )
        .await
    }
}
