//! MCP 官方 HTTP 传输和 OAuth 共用产品网络策略；每次外呼读取当前策略代次。
use futures::{stream::BoxStream, StreamExt};
use lingxi_adapters::models::{
    dispatch::HttpTimeouts,
    network::{NetworkClient, NetworkPlane},
};
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::{
        auth::{OAuthHttpClient, OAuthHttpClientError, OAuthHttpClientFuture, OAuthHttpRequest},
        streamable_http_client::{
            SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct NetworkMcpClient {
    client: NetworkClient,
}
impl NetworkMcpClient {
    pub(crate) fn new(plane: &Arc<NetworkPlane>) -> Result<Self, String> {
        plane
            .client_handle(HttpTimeouts::default())
            .map(|client| Self { client })
            .map_err(|_| "MCP 网络策略客户端不可用".to_owned())
    }
    fn client(&self) -> Result<reqwest::Client, StreamableHttpError<reqwest::Error>> {
        self.client
            .client()
            .map_err(|_| StreamableHttpError::Io(std::io::Error::other("MCP 网络策略更新失败")))
    }
}
impl StreamableHttpClient for NetworkMcpClient {
    type Error = reqwest::Error;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.post_message_with_max_sse_event_size(
            uri,
            message,
            session_id,
            auth_header,
            headers,
            1024 * 1024,
        )
        .await
    }
    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        cap: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.client()?
            .post_message_with_max_sse_event_size(
                uri,
                message,
                session_id,
                auth_header,
                headers,
                cap,
            )
            .await
    }
    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        self.client()?
            .delete_session(uri, session_id, auth_header, headers)
            .await
    }
    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<
        BoxStream<'static, Result<sse_stream::Sse, SseError>>,
        StreamableHttpError<Self::Error>,
    > {
        self.get_stream_with_max_sse_event_size(
            uri,
            session_id,
            last_event_id,
            auth_header,
            headers,
            1024 * 1024,
        )
        .await
    }
    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        cap: usize,
    ) -> Result<
        BoxStream<'static, Result<sse_stream::Sse, SseError>>,
        StreamableHttpError<Self::Error>,
    > {
        self.client()?
            .get_stream_with_max_sse_event_size(
                uri,
                session_id,
                last_event_id,
                auth_header,
                headers,
                cap,
            )
            .await
    }
}
impl OAuthHttpClient for NetworkMcpClient {
    fn execute(&self, operation: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            // 共享策略客户端本来就不自动重定向，SDK负责受约束的发现重定向。
            let client = self.client.client().map_err(|_| {
                Box::new(std::io::Error::other("MCP OAuth 网络策略不可用")) as OAuthHttpClientError
            })?;
            let request: reqwest::Request = operation
                .request
                .try_into()
                .map_err(|error| Box::new(error) as OAuthHttpClientError)?;
            let response = client
                .execute(request)
                .await
                .map_err(|error| Box::new(error) as OAuthHttpClientError)?;
            let mut builder = axum::http::Response::builder()
                .status(response.status())
                .version(response.version());
            for (key, value) in response.headers() {
                builder = builder.header(key, value);
            }
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| Box::new(error) as OAuthHttpClientError)?;
                if body.len().saturating_add(chunk.len()) > 1024 * 1024 {
                    return Err(Box::new(std::io::Error::other("MCP OAuth 响应超过1MiB"))
                        as OAuthHttpClientError);
                }
                body.extend_from_slice(&chunk);
            }
            builder
                .body(body)
                .map_err(|error| Box::new(error) as OAuthHttpClientError)
        })
    }
}
