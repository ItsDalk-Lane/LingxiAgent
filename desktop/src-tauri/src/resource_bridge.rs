use serde_json::{json, Value};
use std::{net::SocketAddr, path::Path, time::Duration};

/// 凭证留在宿主；只能访问现行 Rust 服务的三个资源监听入口。
pub struct ResourceConnection {
    origin: String,
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
        let origin = format!("http://{addr}");
        Ok(Self {
            base: format!("{origin}/lingxi/v1/resource-io"),
            origin,
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

    /// R09-T03：首次设置完成（POST setup-complete；返回服务确认的 setupComplete）。
    pub async fn setup_complete(&self) -> Result<Value, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .post(format!("{}/lingxi/v1/preferences/setup-complete", self.origin))
            .bearer_auth(&self.token)
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| "无法连接本机设置服务".to_string())?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|_| "设置服务返回无效数据".to_string())?;
        if !status.is_success() {
            return Err(format!(
                "设置服务拒绝（{}）：{}",
                status.as_u16(),
                value["error"].as_str().unwrap_or("保存失败")
            ));
        }
        Ok(value)
    }

    /// R09-T03：DOCX/XLSX 转 HTML（Electron `read-docx-html`/`read-xlsx-html`
    /// 的 Rust 服务等价物；走本机服务受限 fs 授权根，返回真实 HTML 文本）。
    pub async fn office_html(&self, kind: &str, file_path: &str) -> Result<Option<String>, String> {
        if kind != "docx" && kind != "xlsx" {
            return Err("不支持的 Office 预览类型".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let mut url = reqwest::Url::parse(&format!("{}/lingxi/v1/fs/{kind}-html", self.origin))
            .map_err(|_| "本机文档服务地址无效".to_string())?;
        url.query_pairs_mut().append_pair("path", file_path);
        let response = client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| "无法连接本机文档服务".to_string())?;
        let status = response.status();
        if !status.is_success() {
            // 与 Electron 一致：解析失败/无效输入返回 null（不伪造 HTML）。
            return Ok(None);
        }
        let html = response
            .text()
            .await
            .map_err(|_| "文档服务返回无效数据".to_string())?;
        Ok(Some(html))
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
