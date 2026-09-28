//! 浏览器网页资源入口：只读取明确指定的构建目录，路径在规范化后仍必须留在目录内。

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as RoutePath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::ServiceState;

#[derive(Clone, Debug)]
pub(crate) enum StaticWebConfig {
    Dist(PathBuf),
    Guide,
    Error,
}

impl StaticWebConfig {
    pub(crate) fn resolve() -> Self {
        if let Some(injected) = std::env::var_os("LINGXI_RENDERER_DIST") {
            let root = PathBuf::from(injected);
            return match valid_dist(&root) {
                Some(root) => Self::Dist(root),
                None => Self::Error,
            };
        }
        let dev = std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join("desktop/dist-renderer"));
        dev.and_then(|root| valid_dist(&root).map(Self::Dist))
            .unwrap_or(Self::Guide)
    }
}

fn valid_dist(path: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(path).ok()?;
    root.join("mobile.html").is_file().then_some(root)
}

pub(crate) fn routes() -> Router<ServiceState> {
    Router::new()
        .route("/mobile", get(mobile_index))
        .route("/mobile/", get(mobile_index))
        .route("/mobile/{*path}", get(mobile_asset))
        .route("/desktop", get(desktop_index))
        .route("/desktop/", get(desktop_index))
        .route("/desktop/{*path}", get(desktop_asset))
}

async fn mobile_index(State(state): State<ServiceState>) -> Response {
    serve(&state.static_web, "")
}
async fn desktop_index(State(state): State<ServiceState>) -> Response {
    serve(&state.static_web, "")
}
async fn mobile_asset(
    State(state): State<ServiceState>,
    RoutePath(path): RoutePath<String>,
) -> Response {
    serve(&state.static_web, &path)
}
async fn desktop_asset(
    State(state): State<ServiceState>,
    RoutePath(path): RoutePath<String>,
) -> Response {
    serve(&state.static_web, &path)
}

fn serve(config: &StaticWebConfig, request_path: &str) -> Response {
    match config {
        StaticWebConfig::Guide => {
            if !request_path.is_empty() {
                return StatusCode::NOT_FOUND.into_response();
            }
            html(
                StatusCode::OK,
                "网页界面尚未安装。请安装网页构建产物后重启服务。",
            )
        }
        StaticWebConfig::Error => html(
            StatusCode::SERVICE_UNAVAILABLE,
            "网页界面目录缺失或损坏，请检查服务配置。",
        ),
        StaticWebConfig::Dist(root) => {
            let relative = if request_path.is_empty() || request_path == "index.html" {
                "mobile.html"
            } else {
                request_path
            };
            if relative != "mobile.html" && !safe_asset_path(relative) {
                return StatusCode::NOT_FOUND.into_response();
            }
            let file = root.join(relative);
            let Ok(real) = std::fs::canonicalize(&file) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            if !real.starts_with(root) || !real.is_file() {
                return StatusCode::NOT_FOUND.into_response();
            }
            let Ok(meta) = std::fs::metadata(&real) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            if meta.len() > 16 * 1024 * 1024 {
                return StatusCode::PAYLOAD_TOO_LARGE.into_response();
            }
            let Ok(bytes) = std::fs::read(&real) else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            let mime = match real.extension().and_then(|s| s.to_str()).unwrap_or("") {
                "html" => "text/html; charset=utf-8",
                "js" => "text/javascript; charset=utf-8",
                "css" => "text/css; charset=utf-8",
                "json" | "webmanifest" => "application/json; charset=utf-8",
                "svg" => "image/svg+xml",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "ico" => "image/x-icon",
                "woff" => "font/woff",
                "woff2" => "font/woff2",
                "wasm" => "application/wasm",
                _ => "application/octet-stream",
            };
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if relative == "mobile.html" {
                    "no-cache"
                } else {
                    "public, max-age=31536000, immutable"
                }),
            );
            response
        }
    }
}

fn safe_asset_path(path: &str) -> bool {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.contains('%')
    {
        return false;
    }
    let mut parts = path.split('/');
    let first = parts.next().unwrap_or("");
    if !["assets", "icons", "lib", "themes", "locales"].contains(&first)
        && !["manifest.webmanifest", "sw.js", "icon.png"].contains(&path)
    {
        return false;
    }
    path.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn html(status: StatusCode, message: &str) -> Response {
    let mut response = (status, format!("<!doctype html><html lang=\"zh\"><meta charset=\"utf-8\"><title>Lingxi</title><main>{message}</main></html>")).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_path_rejects_traversal_and_unknown_roots() {
        assert!(safe_asset_path("assets/app.js"));
        for path in [
            "../secret",
            "assets/../secret",
            "assets/%2e%2e/secret",
            "assets\\secret",
            "private/file",
            "assets//x",
        ] {
            assert!(!safe_asset_path(path), "{path}");
        }
    }

    #[tokio::test]
    async fn serves_entry_and_asset_with_correct_content_type() {
        let root = std::env::temp_dir().join(format!("lingxi-static-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            root.join("mobile.html"),
            b"<html>current mobile build</html>",
        )
        .unwrap();
        std::fs::write(root.join("assets/app.js"), b"console.log('current')").unwrap();
        let dist = StaticWebConfig::Dist(std::fs::canonicalize(&root).unwrap());
        let page = serve(&dist, "");
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(
            page.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(
            axum::body::to_bytes(page.into_body(), 1024)
                .await
                .unwrap()
                .as_ref(),
            b"<html>current mobile build</html>"
        );
        let asset = serve(&dist, "assets/app.js");
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(
            asset.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            axum::body::to_bytes(asset.into_body(), 1024)
                .await
                .unwrap()
                .as_ref(),
            b"console.log('current')"
        );
        assert_eq!(
            serve(&dist, "assets/../secret").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(serve(&StaticWebConfig::Guide, "").status(), StatusCode::OK);
        assert_eq!(
            serve(&StaticWebConfig::Guide, "assets/app.js").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            serve(&StaticWebConfig::Error, "").status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
