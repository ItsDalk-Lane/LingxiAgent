// T07 E2E 共享 wdio WebDriver 客户端（由 t07_native_e2e.rs include 进来）。
// 语义与 R06 tests/desktop_host.rs 的 Driver 一致：真实 HTTP WebDriver，不做宿主/服务替身。
mod wdio {
    use super::*;

    pub struct Driver {
        client: reqwest::Client,
        base: String,
        session: String,
    }

    impl Driver {
        pub fn new(client: reqwest::Client, base: String, session: &str) -> Self {
            Self {
                client,
                base,
                session: session.to_string(),
            }
        }

        async fn request(&self, method: reqwest::Method, path: &str, body: Value) -> Value {
            let response = self
                .client
                .request(method, format!("{}{}", self.base, path))
                .json(&body)
                .send()
                .await
                .expect("WebDriver HTTP failed");
            let value: Value = response.json().await.expect("WebDriver JSON failed");
            assert!(
                value["value"]["error"].is_null(),
                "WebDriver error: {value}"
            );
            value["value"].clone()
        }

        async fn handles(&self) -> Vec<String> {
            self.request(
                reqwest::Method::GET,
                &format!("/session/{}/window/handles", self.session),
                Value::Null,
            )
            .await
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
        }

        async fn window(&self, handle: &str) {
            self.request(
                reqwest::Method::POST,
                &format!("/session/{}/window", self.session),
                json!({"handle": handle}),
            )
            .await;
        }

        pub async fn script(&self, script: &str, args: Value, asynchronous: bool) -> Value {
            self.request(
                reqwest::Method::POST,
                &format!(
                    "/session/{}/execute/{}",
                    self.session,
                    if asynchronous { "async" } else { "sync" }
                ),
                json!({"script": script, "args": args}),
            )
            .await
        }

        pub async fn wait_script(&self, script: &str, args: Value, limit: Duration) -> Value {
            let started = Instant::now();
            loop {
                let result = self.script(script, args.clone(), false).await;
                if result != Value::Null && result != Value::Bool(false) {
                    return result;
                }
                assert!(started.elapsed() < limit, "界面未到达要求状态：{script}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        /// 选择产品主窗（react-root + Rust 预览标题）；不是 R06 资源窗口。
        pub async fn pick_product_window(&self) -> String {
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                for handle in self.handles().await {
                    self.window(&handle).await;
                    let probe = self
                        .script(
                            "return document.title + '|' + (!!document.getElementById('react-root'));",
                            json!([]),
                            false,
                        )
                        .await;
                    if probe
                        .as_str()
                        .map(|text| text.contains("Rust 预览") && text.ends_with("true"))
                        .unwrap_or(false)
                    {
                        return handle;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "产品主窗未出现（title==='Lingxi · Rust 预览' 且 #react-root 已挂载）"
                );
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }

        pub async fn screenshot(&self) -> String {
            let value = self
                .request(
                    reqwest::Method::GET,
                    &format!("/session/{}/screenshot", self.session),
                    Value::Null,
                )
                .await;
            value.as_str().unwrap_or_default().to_string()
        }
    }
}
