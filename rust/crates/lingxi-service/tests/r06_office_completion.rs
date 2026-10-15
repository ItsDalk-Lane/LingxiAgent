//! 从当前真实 HTTP 路由读取合成 Office 字节，成功与拒绝都检查原文件保全。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::json;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Host {
    root: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}
impl Host {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "r06-office-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let home = root.join("home");
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home,
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
            },
            &layout,
            ServiceDeps {
                workspace_root: Some(workspace.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            run(
                serving,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        Self {
            root,
            workspace,
            state,
            base: format!("http://{}", addr.await.unwrap()),
            stop,
            task,
        }
    }
    async fn get(&self, kind: &str, path: &std::path::Path, auth: bool) -> (u16, String) {
        let encoded = percent_encoding::utf8_percent_encode(
            path.to_str().unwrap(),
            percent_encoding::NON_ALPHANUMERIC,
        );
        let auth = if auth {
            format!(
                "Authorization: Bearer {}\r\n",
                self.state.auth().local_token()
            )
        } else {
            String::new()
        };
        let address = self.base.strip_prefix("http://").unwrap();
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let wire=format!("GET /lingxi/v1/fs/{kind}?path={encoded} HTTP/1.1\r\nHost: {address}\r\n{auth}Connection: close\r\n\r\n");
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            stream.read_to_end(&mut raw),
        )
        .await
        .unwrap()
        .unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&raw[..split]).unwrap();
        let status = header
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (
            status,
            String::from_utf8(raw[split + 4..].to_vec()).unwrap(),
        )
    }
    async fn finish(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap().unwrap();
    }
}
fn package(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in parts {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn word(text: &str) -> Vec<u8> {
    package(&[(
        "word/document.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>表格原文</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
        ),
    )])
}
fn workbook() -> Vec<u8> {
    package(&[
        (
            "xl/workbook.xml",
            r#"<workbook xmlns:r="rel"><sheets><sheet name="First" sheetId="1" r:id="chosen"/><sheet name="Second" sheetId="2" r:id="other"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="other" Type="urn:/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="chosen" Type="urn:/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<sst><si><t>第一张 &lt;script&gt;alert(1)&lt;/script&gt; &amp; 原文</t></si><si><r><t>富文本</t></r><r><t>尾部</t></r></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>不许返回第二张</t></is></c></row></sheetData></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            r#"<worksheet><sheetData><row r="2"><c r="A2" t="s"><v>0</v></c><c r="C2"><v>42</v></c></row><row r="4"><c r="B4" t="s"><v>1</v></c><c r="C4" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
        ),
    ])
}

fn dated_workbook(calendar: bool, format_id: u32, format: &str, value: &str) -> Vec<u8> {
    package(&[
        (
            "xl/workbook.xml",
            &format!(
                r#"<workbook xmlns:r="rel"><workbookPr date1904="{calendar}"/><sheets><sheet name="日期" r:id="first"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships><Relationship Id="first" Type="urn:/worksheet" Target="worksheets/first.xml"/></Relationships>"#,
        ),
        (
            "xl/styles.xml",
            &format!(
                r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="{format}"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="{format_id}"/></cellXfs></styleSheet>"#
            ),
        ),
        (
            "xl/worksheets/first.xml",
            &format!(
                r#"<worksheet><sheetData><row><c r="A1" s="1"><v>{value}</v></c></row></sheetData></worksheet>"#
            ),
        ),
    ])
}

#[tokio::test]
async fn real_excel_dates_keep_time_in_both_calendars_and_never_show_serials() {
    let h = Host::new().await;
    for (calendar, id, format, serial, expected) in [
        (
            false,
            164,
            "yyyy-mm-dd hh:mm:ss",
            "46309.39583333333",
            "2026-10-14T09:30:00.000Z",
        ),
        (
            true,
            164,
            "yyyy-mm-dd hh:mm:ss",
            "44847.39583333333",
            "2026-10-14T09:30:00.000Z",
        ),
        (
            false,
            14,
            "0",
            "46309.39583333333",
            "2026-10-14T09:30:00.000Z",
        ),
        (false, 164, "[Red]&quot;day&quot; 0.00", "42", "42"),
        (false, 164, "[h]:mm:ss", "0.5", "1899-12-30T12:00:00.000Z"),
    ] {
        let bytes = dated_workbook(calendar, id, format, serial);
        let file = h
            .workspace
            .join(format!("dates-{calendar}-{id}-{}.xlsx", format.len()));
        std::fs::write(&file, &bytes).unwrap();
        let response = h.get("xlsx-html", &file, true).await;
        assert_eq!(response.0, 200, "{response:?}");
        assert_eq!(
            response.1,
            format!("<table><tr><td>{expected}</td></tr></table>")
        );
        assert_eq!(std::fs::read(file).unwrap(), bytes);
    }
    let invalid = dated_workbook(false, 164, "yyyy-mm-dd", "NaN");
    assert!(lingxi_service::office::xlsx(&invalid).is_err());
    h.finish().await;
}

#[tokio::test]
async fn current_docx_html_route_converts_actual_archive_and_preserves_file() {
    let h = Host::new().await;
    let file = h.workspace.join("word.docx");
    let bytes = word("实际中文 &amp; &lt;script&gt;拒绝脚本&lt;/script&gt; &#x1F642;");
    std::fs::write(&file, &bytes).unwrap();
    let response = h.get("docx-html", &file, true).await;
    assert_eq!(response.0, 200, "{response:?}");
    assert!(response
        .1
        .contains("<h1>实际中文 &amp; &lt;script&gt;拒绝脚本&lt;/script&gt; 🙂</h1>"));
    assert!(response
        .1
        .contains("<table><tr><td><p>表格原文</p></td></tr></table>"));
    assert!(!response.1.contains("<script>"));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    h.finish().await;
}
#[tokio::test]
async fn current_xlsx_html_route_uses_first_relationship_and_real_cells() {
    let h = Host::new().await;
    let file = h.workspace.join("book.xlsx");
    let bytes = workbook();
    std::fs::write(&file, &bytes).unwrap();
    let response = h.get("xlsx-html", &file, true).await;
    assert_eq!(response.0, 200, "{response:?}");
    assert_eq!(response.1,"<table><tr><td>第一张 &lt;script&gt;alert(1)&lt;/script&gt; &amp; 原文</td><td></td><td>42</td></tr><tr><td></td><td>富文本尾部</td><td>true</td></tr></table>");
    assert!(!response.1.contains("不许返回第二张"));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    h.finish().await;
}
#[tokio::test]
async fn office_http_refusals_are_visible_and_never_publish_or_modify_files() {
    let h = Host::new().await;
    let outside = h.root.join("private.docx");
    std::fs::write(&outside, word("路径外秘密，不得泄漏")).unwrap();
    let corrupt = h.workspace.join("bad.xlsx");
    std::fs::write(&corrupt, b"not a zip").unwrap();
    let missing = h.workspace.join("missing.docx");
    let large = h.workspace.join("large.docx");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(20 * 1024 * 1024 + 1)
        .unwrap();
    for kind in ["docx-html", "xlsx-html"] {
        let denied = h.get(kind, &outside, true).await;
        assert_eq!(denied.0, 403, "{denied:?}");
        assert!(!denied.1.contains("路径外秘密"));
        assert_eq!(h.get(kind, &outside, false).await.0, 401);
        assert_eq!(h.get(kind, &missing, true).await.0, 404);
        assert_eq!(h.get(kind, &h.workspace, true).await.0, 400);
        assert_eq!(h.get(kind, &large, true).await.0, 413);
        let bad = h.get(kind, &corrupt, true).await;
        assert_eq!(bad.0, 500);
        assert!(bad.1.contains("parse failed"));
        #[cfg(unix)]
        {
            let link = h.workspace.join(format!("link-{kind}"));
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let denied = h.get(kind, &link, true).await;
            assert_eq!(denied.0, 403);
            assert!(!denied.1.contains("路径外秘密"));
        }
    }
    assert_eq!(std::fs::read(&corrupt).unwrap(), b"not a zip");
    assert!(missing.symlink_metadata().is_err());
    assert_eq!(
        std::fs::metadata(&large).unwrap().len(),
        20 * 1024 * 1024 + 1
    );
    h.finish().await;
}
#[test]
fn office_xml_external_entities_and_expansion_limits_are_explicit_refusals() {
    let dtd = package(&[(
        "word/document.xml",
        r#"<!DOCTYPE document [<!ENTITY ext SYSTEM "file:///etc/passwd">]><document><body><p><t>&ext;</t></p></body></document>"#,
    )]);
    assert!(lingxi_service::office::docx(&dtd)
        .err()
        .unwrap()
        .contains("document types"));
    let corrupt = package(&[(
        "word/document.xml",
        "<document><body><p><t>正文</p></body></document>",
    )]);
    assert!(lingxi_service::office::docx(&corrupt).is_err());
    let huge = format!(
        "<document><body>{}</body></document>",
        " ".repeat(20 * 1024 * 1024)
    );
    assert!(
        lingxi_service::office::docx(&package(&[("word/document.xml", &huge)]))
            .err()
            .unwrap()
            .contains("limit")
    );
    assert_eq!(
        lingxi_service::office::docx(&word("真实&amp;文本"))
            .unwrap()
            .text,
        "真实&文本\n表格原文\n\t\n"
    );
    let parsed = lingxi_service::office::xlsx(&workbook()).unwrap();
    assert!(parsed
        .text
        .contains("第一张 <script>alert(1)</script> & 原文\t\t42"));
    assert!(parsed.text.contains("富文本尾部\ttrue"));
    println!(
        "{}",
        json!({"onlyArchiveBytes":true,"externalEntityRefused":true,"sourceTextRetained":true})
    );
}

#[test]
fn current_native_office_preview_and_visible_real_rust_errors() {
    assert_eq!(std::env::consts::OS, "macos", "本真实窗口检查仅代表macOS");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let root = std::env::temp_dir().join(format!(
        "r06-office-ui-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixtures = root.join("fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    std::fs::write(
        fixtures.join("actual.docx"),
        word("实际中文 &amp; &lt;script&gt;拒绝脚本&lt;/script&gt;"),
    )
    .unwrap();
    std::fs::write(fixtures.join("actual.xlsx"), workbook()).unwrap();
    std::fs::write(
        fixtures.join("actual-dates.xlsx"),
        dated_workbook(false, 164, "yyyy-mm-dd hh:mm:ss", "46309.39583333333"),
    )
    .unwrap();
    for name in ["broken.docx", "broken.xlsx"] {
        std::fs::write(fixtures.join(name), b"actual broken archive").unwrap();
    }
    let node = std::env::var_os("R06_NODE_BIN").unwrap_or_else(|| "node".into());
    let build = std::process::Command::new(&node)
        .arg(repo.join("node_modules/vite/bin/vite.js"))
        .args(["build", "--config", "vite.config.preload.js"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(build.status.success(), "当前preload必须实际构建");
    let output = std::process::Command::new(node)
        .arg(repo.join("desktop/src/react/__tests__/office-live/launch.mjs"))
        .current_dir(&repo)
        .env("R06_OFFICE_HOME", &root)
        .env("R06_OFFICE_BIN", env!("CARGO_BIN_EXE_lingxi-service"))
        .output()
        .unwrap();
    std::fs::write(root.join("stdout.log"), &output.stdout).unwrap();
    std::fs::write(root.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "真实Office界面失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "PASS_MAC_CURRENT_RUST_OFFICE_UI");
    assert_eq!(receipt["sourcePreserved"], true);
    assert_eq!(receipt["failed"].as_array().unwrap().len(), 2);
    println!(
        "OFFICE_UI_CURRENT_RECEIPT {}",
        root.join("result.json").display()
    );
}
