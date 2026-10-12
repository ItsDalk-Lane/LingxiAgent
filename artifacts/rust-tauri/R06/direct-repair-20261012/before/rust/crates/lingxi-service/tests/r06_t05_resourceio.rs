//! R06-T05 Round 4 生产链测试：ResourceIO 内核 / provider 集合 / 事件总线 /
//! 轮询 watch 注册表 + 15 路由面（叶 #31-45、#53 份额）。
//!
//! 对照纪律（RC-3，逐条亲读）：
//! - 内核：`lib/resource-io/resource-io.ts`（callProvider 能力闸 :202-224、
//!   跨 provider move 拒绝 :180-193、emitChanged/Deleted/Renamed）。
//! - ref 归一：`lib/resource-io/resource-refs.ts`（kind 同义词 `_`→`-` 小写、
//!   resource/ref/target 嵌套解包、推断序 url>fileId>resourceId>mountId>path、
//!   resourceKey 五形态）。
//! - 错误词汇：`lib/resource-io/errors.ts`（capability_denied 403 /
//!   provider_not_available 501 / resource_access_denied 403 /
//!   cross_provider_*_unsupported 501 / resource_not_found 404 /
//!   target_already_exists 409 / invalid_trash_namespace 400）。
//! - local_fs provider：`providers/local-fs-provider.ts`（SEARCH_SKIP_DIRS、
//!   versionFromStat、fileVersionsMatch :384-391、trash_{ms}_{4hex} +
//!   metadata.json schemaVersion 1、realOrResolved 上溯）。
//! - session_file/resource provider：`providers/session-file-resolver.ts` +
//!   `session-file-resolver.ts`（400 invalid_resource_ref / 404
//!   resource_not_found / 410 resource_expired / 500 invalid_resource_path /
//!   载荷缺失 404）与 `providers/resource-provider.ts`（stat/read/
//!   materialize 只读，写入一律 capability_denied）。
//! - 事件总线：`resource-event-bus.ts`（dedupeSize 512 FIFO、retention
//!   1000、changed 去重键 = version 字段 JSON、since 游标 < 最旧-1 → stale）。
//! - watch：`resource-watch-registry.ts`（refcount、UUID 订阅、诊断面、
//!   80ms 合并窗口）——机制差异 D7：fs.watch → mtime/size 轮询。
//! - 路由：`server/routes/resource-io.ts`（15 叶面、resourceJson 409 冲突
//!   外发、encoding base64/utf-8、invalid_resource_encoding 400）。
//! - 设计登记：D7（轮询 watch）、D9/J2（mount/url → unsupported_provider
//!   400 显式拒绝）、D12（查询严格化：events 只认 since，畸形/未知键 400）。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::resourceio::{
    normalize_resource_ref, provider_id_for_ref, resource_key_for_ref, OpContext, ResourceEventBus,
    ResourceIoService, ResourceRef,
};
use lingxi_service::resources::{resource_id_for_session_file_id, ResourceService};
use lingxi_service::sessionfiles::{SessionFileService, SESSION_FILE_CACHE_INACTIVE_TTL_MS};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 纯函数：ref 归一 / resourceKey / provider 映射 ──

#[test]
fn normalize_resource_ref_golden_table() {
    // kind 同义词：`_`→`-` + 小写（resource-refs.ts :8-10）。
    let r = normalize_resource_ref(&serde_json::json!({"kind": "local_file", "path": "/a/b"}))
        .expect("local_file synonym");
    assert!(matches!(r, ResourceRef::LocalFile { ref path } if path == "/a/b"));
    let r = normalize_resource_ref(&serde_json::json!({"kind": "LOCAL-PATH", "path": "/a"}))
        .expect("LOCAL-PATH synonym");
    assert!(matches!(r, ResourceRef::LocalFile { .. }));
    // type 兜底 kind（:26）。
    let r = normalize_resource_ref(&serde_json::json!({"type": "path", "path": "/a"}))
        .expect("type synonym");
    assert!(matches!(r, ResourceRef::LocalFile { .. }));
    // session-file 的 id 兜底（kind 命中时 value.id 可作 fileId，:32）。
    let r = normalize_resource_ref(&serde_json::json!({"kind": "session-file", "id": "sf_x9"}))
        .expect("id fallback");
    match r {
        ResourceRef::SessionFile {
            file_id,
            session_id,
        } => {
            assert_eq!(file_id, "sf_x9");
            assert_eq!(session_id, None);
        }
        other => panic!("expected session-file, got {other:?}"),
    }
    // sessionId 透传。
    let r = normalize_resource_ref(
        &serde_json::json!({"kind": "session_file", "fileId": "sf_a", "sessionId": "s-1"}),
    )
    .expect("sessionId carry");
    match r {
        ResourceRef::SessionFile {
            file_id,
            session_id,
        } => {
            assert_eq!(file_id, "sf_a");
            assert_eq!(session_id.as_deref(), Some("s-1"));
        }
        other => panic!("expected session-file, got {other:?}"),
    }
    // resource 的 id 兜底（:34）。
    let r = normalize_resource_ref(&serde_json::json!({"kind": "resource", "id": "res_sf_a"}))
        .expect("resource id fallback");
    assert!(matches!(r, ResourceRef::Resource { ref resource_id } if resource_id == "res_sf_a"));
    // mount 的 rootId 同义（:36）。
    let r = normalize_resource_ref(
        &serde_json::json!({"kind": "mount", "rootId": "m1", "path": "a/b"}),
    )
    .expect("mount rootId");
    match r {
        ResourceRef::Mount { mount_id, path } => {
            assert_eq!(mount_id, "m1");
            assert_eq!(path, "a/b");
        }
        other => panic!("expected mount, got {other:?}"),
    }
    // url 的 href 同义（:35）。
    let r = normalize_resource_ref(&serde_json::json!({"kind": "url", "href": "https://x/y"}))
        .expect("href synonym");
    assert!(matches!(r, ResourceRef::Url { ref url } if url == "https://x/y"));
    // 嵌套解包：resource / ref / target（:16-24）。
    for wrapper in ["resource", "ref", "target"] {
        let r = normalize_resource_ref(
            &serde_json::json!({wrapper: {"kind": "url", "url": "https://nested"}}),
        )
        .expect("nested unwrap");
        assert!(matches!(r, ResourceRef::Url { ref url } if url == "https://nested"));
    }
    // 推断序 url > fileId > resourceId > mountId > path（:64-68）。
    let r = normalize_resource_ref(
        &serde_json::json!({"url": "https://u", "fileId": "sf_b", "path": "/p"}),
    )
    .expect("url wins");
    assert!(matches!(r, ResourceRef::Url { .. }));
    let r = normalize_resource_ref(&serde_json::json!({"fileId": "sf_b", "resourceId": "res_c"}))
        .expect("fileId wins over resourceId");
    assert!(matches!(r, ResourceRef::SessionFile { .. }));
    let r = normalize_resource_ref(&serde_json::json!({"resourceId": "res_c", "mountId": "m"}))
        .expect("resourceId wins over mountId");
    assert!(matches!(r, ResourceRef::Resource { .. }));
    let r = normalize_resource_ref(&serde_json::json!({"mountId": "m", "path": "/p"}))
        .expect("mountId wins over path");
    assert!(matches!(r, ResourceRef::Mount { .. }));
    let r = normalize_resource_ref(&serde_json::json!({"path": "/p"})).expect("path inference");
    assert!(matches!(r, ResourceRef::LocalFile { .. }));
    // 缺必填 → 400 invalid_resource_ref（D12：候选人给稳定 code；现役为
    // 无 code 的 Error，路由同样 400 —— 台账登记）。
    for bad in [
        serde_json::json!(null),
        serde_json::json!(42),
        serde_json::json!("str"),
        serde_json::json!({}),
        serde_json::json!({"kind": "local-file"}),
        serde_json::json!({"kind": "session-file"}),
        serde_json::json!({"kind": "mount"}),
        serde_json::json!({"kind": "resource"}),
        serde_json::json!({"kind": "url"}),
        serde_json::json!({"kind": "alien", "foo": "bar"}),
    ] {
        let err = normalize_resource_ref(&bad).unwrap_err();
        assert_eq!(err.status, 400, "bad ref {bad}");
        assert_eq!(err.code, "invalid_resource_ref");
    }
}

#[test]
fn resource_keys_and_provider_ids() {
    // resourceKeyForRef（resource-refs.ts :72-85）五形态。
    assert_eq!(
        resource_key_for_ref(&ResourceRef::LocalFile {
            path: "/tmp/x".to_string()
        }),
        "local_fs:/tmp/x"
    );
    assert_eq!(
        resource_key_for_ref(&ResourceRef::SessionFile {
            file_id: "sf_a".to_string(),
            session_id: None,
        }),
        "session_file:sf_a"
    );
    assert_eq!(
        resource_key_for_ref(&ResourceRef::Resource {
            resource_id: "res_sf_a".to_string()
        }),
        "resource:res_sf_a"
    );
    // mount 路径归一：反斜杠→斜杠、去首尾斜杠（:12-14, :77）。
    assert_eq!(
        resource_key_for_ref(&ResourceRef::Mount {
            mount_id: "m1".to_string(),
            path: "\\a\\b\\".to_string(),
        }),
        "mount:m1:a/b"
    );
    assert_eq!(
        resource_key_for_ref(&ResourceRef::Url {
            url: "https://u".to_string()
        }),
        "url:https://u"
    );
    // providerIdForResourceRef（:87-100）。
    assert_eq!(
        provider_id_for_ref(&ResourceRef::LocalFile {
            path: "/x".to_string()
        }),
        "local_fs"
    );
    assert_eq!(
        provider_id_for_ref(&ResourceRef::SessionFile {
            file_id: "sf_a".to_string(),
            session_id: None,
        }),
        "session_file"
    );
    assert_eq!(
        provider_id_for_ref(&ResourceRef::Resource {
            resource_id: "r".to_string()
        }),
        "resource"
    );
    assert_eq!(
        provider_id_for_ref(&ResourceRef::Mount {
            mount_id: "m".to_string(),
            path: String::new(),
        }),
        "mount"
    );
    assert_eq!(
        provider_id_for_ref(&ResourceRef::Url {
            url: "u".to_string()
        }),
        "url"
    );
}

// ── 内核 / provider 级（不起 HTTP） ──

struct IoFixture {
    home: PathBuf,
    payload_dir: PathBuf,
    workspace: PathBuf,
    sf: Arc<SessionFileService>,
    io: ResourceIoService,
}

impl Drop for IoFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.payload_dir);
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

async fn io_fixture(tag: &str, now_ms: u64) -> IoFixture {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-io-{uniq}"));
    let payload_dir = std::env::temp_dir().join(format!("lingxi-r06t05-io-payload-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-io-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&payload_dir);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&payload_dir).expect("mkdir payload");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // macOS：temp_dir 的 /var → /private/var 符号链接。实现侧
    // （ResourceAccess / realpath）产出的 resourceKey/filePath 均为真实
    // 路径，夹具统一规范化后再断言，避免路径拼写假差异。
    let home = home.canonicalize().expect("canonical home");
    let payload_dir = payload_dir.canonicalize().expect("canonical payload");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let db = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            &home.join("runs.db"),
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("open db"),
    );
    let sf = Arc::new(SessionFileService::new(home.clone(), db, move || now_ms));
    let resources = Arc::new(ResourceService::new(
        sf.clone(),
        "studio-test".to_string(),
        home.clone(),
        move || now_ms,
    ));
    let io = ResourceIoService::new(
        sf.clone(),
        resources,
        home.clone(),
        // 与 bootstrap 同形态：共享的 ResourceAccess 实例 + workspace 根。
        Some((
            Arc::new(
                lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(
                    &workspace,
                ))
                .expect("workspace access"),
            ),
            workspace.clone(),
        )),
        40, // watch_poll_ms：测试用快轮询
        Arc::new(move || now_ms),
    );
    IoFixture {
        home,
        payload_dir,
        workspace,
        sf,
        io,
    }
}

fn ctx() -> OpContext {
    OpContext::local_owner("s-io")
}

#[tokio::test]
async fn kernel_cross_provider_move_rejected_loudly() {
    let fx = io_fixture("xmove", 1_000).await;
    // 跨 provider 移动必须拒绝（resource-io.ts :180-193 →
    // cross_provider_move_unsupported 501）。
    let from = serde_json::json!({"kind": "local-file", "path": "/tmp/a"});
    let to = serde_json::json!({"kind": "session-file", "fileId": "sf_b"});
    let err = fx.io.move_resource(&from, &to, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (501, "cross_provider_move_unsupported")
    );
    let err = fx.io.rename(&to, &from, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (501, "cross_provider_move_unsupported")
    );
}

#[tokio::test]
async fn mount_and_url_refs_get_explicit_unsupported_provider() {
    // D9/J2：mount/url provider 缺席 → unsupported_provider 400 响亮拒绝，
    // 绝不静默走本地路径。
    let fx = io_fixture("d9", 1_000).await;
    let mount = serde_json::json!({"kind": "mount", "mountId": "m1", "path": "a/b"});
    let url = serde_json::json!({"kind": "url", "url": "https://example.com/x"});
    let err = fx.io.stat(&mount, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "unsupported_provider")
    );
    let err = fx.io.read(&url, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "unsupported_provider")
    );
    let err = fx.io.materialize(&mount, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "unsupported_provider")
    );
    let err = fx.io.write(&url, b"x".to_vec(), &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "unsupported_provider")
    );
    // subscribe 路径同样响亮（watch 目标解析走同一 provider 闸）。
    let err = fx
        .io
        .subscribe(&serde_json::json!({"resource": mount}))
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "unsupported_provider")
    );
}

#[tokio::test]
async fn local_fs_stat_read_write_list_search_rename_move_trash_chain() {
    let fx = io_fixture("local", 1_000).await;
    let target = fx.workspace.join("sub").join("note.txt");
    let target_ref = serde_json::json!({"kind": "local-file", "path": target.to_str().unwrap()});

    // stat 缺失 → exists:false（不报错，local-fs-provider.ts :79-87）。
    let stat = fx.io.stat(&target_ref, &ctx()).await.expect("stat missing");
    assert_eq!(stat["exists"], false);
    assert_eq!(stat["isDirectory"], false);
    assert_eq!(
        stat["resourceKey"],
        format!("local_fs:{}", target.to_str().unwrap())
    );

    // write：父目录递归创建，changeType created（:113-120）。
    let out = fx
        .io
        .write(&target_ref, b"hello world".to_vec(), &ctx())
        .await
        .expect("write");
    assert_eq!(out["changeType"], "created");
    assert!(out["version"]["mtimeMs"].is_u64());
    assert_eq!(out["version"]["size"], 11);

    // 再写 → modified。
    let out = fx
        .io
        .write(&target_ref, b"hello again\nsecond line".to_vec(), &ctx())
        .await
        .expect("rewrite");
    assert_eq!(out["changeType"], "modified");

    // stat → version {mtimeMs, size}。
    let stat = fx.io.stat(&target_ref, &ctx()).await.expect("stat");
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["version"]["size"], 23);
    let version = stat["version"].clone();

    // read → 字节级一致 + version（:99-111）。
    let read = fx.io.read(&target_ref, &ctx()).await.expect("read");
    assert_eq!(read.content, b"hello again\nsecond line");
    assert_eq!(read.meta["version"]["size"], 23);

    // write-expected-version：版本吻合 → 写；失配 → 409 冲突结果（不是
    // 异常），且 currentVersion 回带（:122-137）。
    let ok = fx
        .io
        .write_expected_version(&target_ref, b"v2".to_vec(), &version, &ctx())
        .await
        .expect("wev ok");
    assert_eq!(ok["changeType"], "modified");
    let conflict = fx
        .io
        .write_expected_version(&target_ref, b"v3".to_vec(), &version, &ctx())
        .await
        .expect("wev conflict is a result");
    assert_eq!(conflict["ok"], false);
    assert_eq!(conflict["conflict"], true);
    assert_eq!(conflict["version"]["size"], 2);
    // 冲突不覆盖。
    assert_eq!(std::fs::read(&target).expect("read back"), b"v2");

    // list：按名排序，目录 size null（:272-292）。
    std::fs::write(fx.workspace.join("aaa.txt"), b"a").expect("write aaa");
    let list = fx
        .io
        .list(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.to_str().unwrap()}),
            &ctx(),
        )
        .await
        .expect("list");
    let names: Vec<&str> = list["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["aaa.txt", "sub"]);
    assert!(list["items"][1]["isDirectory"].as_bool().unwrap());
    assert!(list["items"][1]["size"].is_null());
    assert_eq!(list["items"][0]["size"], 1);

    // search（text 模式默认）：行号 1 起、命中文本原样（:294-308 +
    // searchText :439-460）；node_modules 跳过（SEARCH_SKIP_DIRS :41）。
    // 注意：wev 步骤已把 note.txt 改写为 "v2"，按当前内容检索。
    std::fs::create_dir_all(fx.workspace.join("node_modules")).expect("mkdir nm");
    std::fs::write(
        fx.workspace.join("node_modules").join("dep.js"),
        b"needle in deps",
    )
    .expect("write dep");
    let found = fx
        .io
        .search(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.to_str().unwrap()}),
            Some("v2".to_string()),
            None,
            None,
            &ctx(),
        )
        .await
        .expect("search text");
    let matches = found["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["line"], 1);
    assert_eq!(matches[0]["text"], "v2");
    // node_modules 内命中被跳过。
    let found = fx
        .io
        .search(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.to_str().unwrap()}),
            Some("needle".to_string()),
            None,
            None,
            &ctx(),
        )
        .await
        .expect("search skip");
    assert_eq!(found["matches"].as_array().unwrap().len(), 0);
    // name 模式（内核面；路由面只透传 query —— 现役同，server/routes/
    // resource-io.ts :94-97）：relativePath 斜杠连接（searchNames
    // :462-515）。
    let found = fx
        .io
        .search(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.to_str().unwrap()}),
            Some("note".to_string()),
            Some("name"),
            None,
            &ctx(),
        )
        .await
        .expect("search name");
    let matches = found["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["name"], "note.txt");
    assert_eq!(matches[0]["relativePath"], "sub/note.txt");
    assert_eq!(matches[0]["parentSubdir"], "sub");
    assert_eq!(matches[0]["isDirectory"], false);

    // rename：新旧 key + 旧路径消失（:189-203 + moveResult :341-350）。
    let renamed = fx.workspace.join("sub").join("renamed.txt");
    let out = fx
        .io
        .rename(
            &target_ref,
            &serde_json::json!({"kind": "local-file", "path": renamed.to_str().unwrap()}),
            &ctx(),
        )
        .await
        .expect("rename");
    assert_eq!(
        out["oldResourceKey"],
        format!("local_fs:{}", target.to_str().unwrap())
    );
    assert_eq!(
        out["newResourceKey"],
        format!("local_fs:{}", renamed.to_str().unwrap())
    );
    assert!(!target.exists() && renamed.exists());
    // 目标已存在 → 409 target_already_exists（:199 + errors.ts :68-75）。
    std::fs::write(fx.workspace.join("aaa.txt"), b"a2").expect("rewrite aaa");
    let err = fx
        .io
        .rename(
            &serde_json::json!({"kind": "local-file", "path": renamed.to_str().unwrap()}),
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.join("aaa.txt").to_str().unwrap()}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (409, "target_already_exists")
    );
    // 源缺失 → 404 resource_not_found（:198）。
    let err = fx
        .io
        .move_resource(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.join("ghost.txt").to_str().unwrap()}),
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.join("dst.txt").to_str().unwrap()}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));

    // trash：trash_{ms}_{4hex} + metadata.json schemaVersion 1（:205-237）。
    let out = fx
        .io
        .trash(
            &serde_json::json!({"kind": "local-file", "path": renamed.to_str().unwrap()}),
            None,
            None,
            &ctx(),
        )
        .await
        .expect("trash");
    let trash_id = out["trashId"].as_str().expect("trashId");
    assert!(trash_id.starts_with("trash_"), "{trash_id}");
    let trash_dir = fx.home.join("trash").join("resource-io").join(trash_id);
    assert_eq!(
        std::fs::read(trash_dir.join("payload")).expect("payload"),
        b"v2"
    );
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(trash_dir.join("metadata.json")).expect("metadata"),
    )
    .expect("metadata json");
    assert_eq!(meta["schemaVersion"], 1);
    assert_eq!(meta["trashId"], trash_id);
    assert_eq!(meta["originalPath"], renamed.to_str().unwrap());
    assert_eq!(meta["originalName"], "renamed.txt");
    assert!(!renamed.exists());
    // 非法命名空间 → 400 invalid_trash_namespace（:393-402）。
    std::fs::write(&target, b"t").expect("write t");
    let err = fx
        .io
        .trash(&target_ref, Some("../escape"), None, &ctx())
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "invalid_trash_namespace")
    );
    // trash 缺失 → 404。
    let err = fx
        .io
        .trash(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.join("ghost.txt").to_str().unwrap()}),
            None,
            None,
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));

    // materialize（叶 #53）：本地直接回真实路径，绝不造假（:239-249）。
    let out = fx
        .io
        .materialize(&target_ref, &ctx())
        .await
        .expect("materialize");
    assert_eq!(out["filePath"], target.to_str().unwrap());
    assert_eq!(out["version"]["size"], 1);

    // 读目录 → 400（:103-104 现役无 code Error；候选人 400 同状态）。
    let err = fx
        .io
        .read(
            &serde_json::json!({"kind": "local-file", "path": fx.workspace.to_str().unwrap()}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.status, 400);

    // 相对路径按 provider cwd（= workspace 根）解析（:310-319）。
    let out = fx
        .io
        .write(
            &serde_json::json!({"kind": "local-file", "path": "rel/note.txt"}),
            b"rel".to_vec(),
            &ctx(),
        )
        .await
        .expect("relative write");
    assert_eq!(out["changeType"], "created");
    assert!(fx.workspace.join("rel").join("note.txt").exists());
}

#[tokio::test]
async fn local_fs_authorization_gate_refuses_outside_workspace() {
    let fx = io_fixture("guard", 1_000).await;
    // 授权闸 = R04-T04 ResourceAccess（设计表 :82）：workspace 外一律
    // resource_access_denied 403（含 safeMessage，errors.ts :27-37）。
    let outside = fx.payload_dir.join("secret.txt");
    std::fs::write(&outside, b"secret").expect("write outside");
    let outside_ref = serde_json::json!({"kind": "local-file", "path": outside.to_str().unwrap()});
    let err = fx.io.read(&outside_ref, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (403, "resource_access_denied")
    );
    assert!(err.safe_message.is_some(), "safeMessage carried");
    let err = fx.io.stat(&outside_ref, &ctx()).await.unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (403, "resource_access_denied")
    );
    let err = fx
        .io
        .write(&outside_ref, b"x".to_vec(), &ctx())
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (403, "resource_access_denied")
    );
    // 真实文件未被触碰（拒绝零副作用）。
    assert_eq!(std::fs::read(&outside).expect("read back"), b"secret");
}

#[tokio::test]
async fn session_file_provider_stat_read_materialize_and_error_chain() {
    // 冷会话清理需要 now > TTL + 余量，夹具时钟取大值。
    let cold_now = SESSION_FILE_CACHE_INACTIVE_TTL_MS + 3_600_000;
    let fx = io_fixture("sfprov", cold_now).await;
    // 注册 external 附件。
    let payload = fx.payload_dir.join("att.txt");
    std::fs::write(&payload, b"attach-bytes").expect("write payload");
    let view = fx
        .sf
        .register_file("s-io", &payload, None, None, "user_upload")
        .await
        .expect("register");
    let sf_ref = serde_json::json!({"kind": "session-file", "fileId": view.file_id});

    // stat：exists + version（session-file-resolver provider :55-66）。
    let stat = fx.io.stat(&sf_ref, &ctx()).await.expect("stat");
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["isDirectory"], false);
    assert_eq!(
        stat["resourceKey"],
        format!("session_file:{}", view.file_id)
    );
    assert_eq!(stat["resource"]["provider"], "session_file");
    assert_eq!(stat["version"]["size"], 12);

    // read：字节级一致（:68-84）。
    let read = fx.io.read(&sf_ref, &ctx()).await.expect("read");
    assert_eq!(read.content, b"attach-bytes");

    // materialize：真实路径（:86-96；叶 #53 不返回假路径）。
    let out = fx
        .io
        .materialize(&sf_ref, &ctx())
        .await
        .expect("materialize");
    assert_eq!(
        out["filePath"],
        payload.canonicalize().unwrap().to_str().unwrap()
    );

    // 写入面一律 capability_denied 403（:98-108 + errors.ts :13-18）。
    for attempt in [
        fx.io.write(&sf_ref, b"x".to_vec(), &ctx()).await,
        fx.io
            .write_expected_version(
                &sf_ref,
                b"x".to_vec(),
                &serde_json::json!({"size": 1}),
                &ctx(),
            )
            .await,
    ] {
        let err = attempt.unwrap_err();
        assert_eq!((err.status, err.code.as_str()), (403, "capability_denied"));
    }

    // 未知 fileId → 404 resource_not_found（session-file-resolver.ts
    // :32-37）。
    let err = fx
        .io
        .stat(
            &serde_json::json!({"kind": "session-file", "fileId": "sf_ghost0000000000"}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));

    // expired → 410 resource_expired（:38-43）：managed_cache 附件 + 冷会话
    // 清理（与 r06_t05_session_files 的冷缓存链同机制）。register_blob 走
    // 上传 MIME 白名单（filemeta：image/audio/video），用 image/png。
    let managed = fx
        .sf
        .register_blob(
            "s-cold",
            Some("v.png"),
            "image/png",
            b"managed",
            "agent_write",
        )
        .await
        .expect("register blob");
    fx.sf
        .mark_session_activity(
            "s-cold",
            cold_now - SESSION_FILE_CACHE_INACTIVE_TTL_MS - 60_000,
        )
        .await
        .expect("mark cold");
    fx.sf.cleanup_cold_sessions().await.expect("cleanup");
    let err = fx
        .io
        .stat(
            &serde_json::json!({"kind": "session-file", "fileId": managed.file_id}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (410, "resource_expired"));

    // 载荷被外部删除 → 404 resource_not_found（payload missing，:62-73）。
    std::fs::remove_file(&payload).expect("remove payload");
    let err = fx.io.read(&sf_ref, &ctx()).await.unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));
}

#[tokio::test]
async fn resource_provider_read_only_over_resource_service() {
    let fx = io_fixture("resprov", 1_000).await;
    let payload = fx.payload_dir.join("doc.txt");
    std::fs::write(&payload, b"doc-body").expect("write payload");
    let view = fx
        .sf
        .register_file("s-io", &payload, None, None, "user_upload")
        .await
        .expect("register");
    let resource_id = resource_id_for_session_file_id(&view.file_id).unwrap();
    let res_ref = serde_json::json!({"kind": "resource", "resourceId": resource_id});

    // stat：version 带 etag（resource-provider.ts :52-66）。
    let stat = fx.io.stat(&res_ref, &ctx()).await.expect("stat");
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["isDirectory"], false);
    assert_eq!(stat["resourceKey"], format!("resource:{resource_id}"));
    let etag = stat["version"]["etag"].as_str().expect("etag");
    assert!(etag.starts_with('"') && etag.ends_with('"'));

    // read / materialize。
    let read = fx.io.read(&res_ref, &ctx()).await.expect("read");
    assert_eq!(read.content, b"doc-body");
    let out = fx
        .io
        .materialize(&res_ref, &ctx())
        .await
        .expect("materialize");
    assert!(out["filePath"].as_str().unwrap().ends_with("doc.txt"));

    // 写入面 capability_denied 403（:97-107）。
    let err = fx
        .io
        .write(&res_ref, b"x".to_vec(), &ctx())
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (403, "capability_denied"));

    // 未知资源 → ResourceService 的 404 resource_not_found 原码透传
    //（normalizeResourceServiceError :137-145）。
    let err = fx
        .io
        .stat(
            &serde_json::json!({"kind": "resource", "resourceId": "res_sf_ghost0000000000"}),
            &ctx(),
        )
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));
}

#[test]
fn event_bus_dedupe_retention_and_since_stale() {
    // resource-event-bus.ts：changed 去重键 = {resourceKey, changeType,
    // version 字段} 的 JSON（:124-136）；dedupeSize/retentionSize 构造可调
    //（:31），默认 512/1000。
    let bus = ResourceEventBus::with_limits(2, 3, Arc::new(|| 1_000));
    let changed = |seq_size: u64| lingxi_service::resourceio::BusChanged {
        change_type: "modified".to_string(),
        resource_key: "local_fs:/x".to_string(),
        resource: serde_json::json!({"kind": "local-file", "path": "/x"}),
        version: Some(serde_json::json!({"mtimeMs": 1, "size": seq_size})),
        source: "api".to_string(),
        reason: None,
        session_path: None,
    };
    // 同版本二次 changed → 去重（None，:43-45）。
    assert_eq!(bus.changed(changed(1)), Some(1));
    assert_eq!(bus.changed(changed(1)), None);
    // 无 version 的 changed 不去重（:125-126 无 version → 无键）。
    let no_version = lingxi_service::resourceio::BusChanged {
        version: None,
        ..changed(9)
    };
    assert!(bus.changed(no_version).is_some());
    // retention 3：已推 3 条（seq 1..3，第二条被去重后实际 emit 2 条 changed
    // + …），再推 deleted/renamed 顶出最旧。
    let _ = bus.deleted(lingxi_service::resourceio::BusDeleted {
        resource_key: "local_fs:/x".to_string(),
        resource: serde_json::json!({"kind": "local-file", "path": "/x"}),
        source: "api".to_string(),
        reason: None,
        session_path: None,
    });
    let last = bus.renamed(lingxi_service::resourceio::BusRenamed {
        old_resource_key: "local_fs:/x".to_string(),
        new_resource_key: "local_fs:/y".to_string(),
        old_resource: serde_json::json!({"kind": "local-file", "path": "/x"}),
        new_resource: serde_json::json!({"kind": "local-file", "path": "/y"}),
        source: "api".to_string(),
        reason: None,
        session_path: None,
    });
    // since：游标 < 最旧-1 → stale + 空事件（:82-99）；否则回送 > 游标的。
    let fresh = bus.since(last - 1);
    assert_eq!(fresh["stale"], false);
    assert_eq!(fresh["latestSequence"], last);
    let events = fresh["events"].as_array().expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "resource.renamed");
    assert_eq!(events[0]["sequence"], last);
    let stale = bus.since(0);
    assert_eq!(stale["stale"], true);
    assert_eq!(stale["events"].as_array().unwrap().len(), 0);
    // 空总线 since 不 stale（:85-87）。
    let empty = ResourceEventBus::new(Arc::new(|| 1_000));
    assert_eq!(empty.since(0)["stale"], false);
}

#[tokio::test]
async fn watch_registry_refcount_diagnostics_and_polling_events() {
    let fx = io_fixture("watch", 1_000).await;
    let watched = fx.workspace.join("watched.txt");
    // 目标缺失 → 响亮 400（现役 fs.watch ENOENT 同步抛出 → 路由 400）。
    let err = fx
        .io
        .watch(&serde_json::json!({"kind": "local-file", "path": watched.to_str().unwrap()}))
        .await
        .unwrap_err();
    assert_eq!(err.status, 400);
    // session_file 无 watch 能力 → 403 capability_denied（resource-io.ts
    // :140-150 resolveWatchTarget 能力闸）。
    let err = fx
        .io
        .watch(&serde_json::json!({"kind": "session-file", "fileId": "sf_x"}))
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (403, "capability_denied"));

    std::fs::write(&watched, b"v1").expect("write watched");
    let key = format!("local_fs:{}", watched.to_str().unwrap());
    // refcount：两次 watch 同一目标 → 一个 entry refCount 2（registry
    // :160-164）。
    let w1 = fx
        .io
        .watch(&serde_json::json!({"kind": "local-file", "path": watched.to_str().unwrap()}))
        .await
        .expect("watch 1");
    let w2 = fx
        .io
        .watch(&serde_json::json!({"kind": "local-file", "path": watched.to_str().unwrap()}))
        .await
        .expect("watch 2");
    assert_ne!(w1, w2);
    let diag = fx.io.diagnostics();
    assert_eq!(diag["watches"].as_array().unwrap().len(), 1);
    assert_eq!(diag["watches"][0]["resourceKey"], key);
    assert_eq!(diag["watches"][0]["refCount"], 2);
    assert_eq!(diag["watches"][0]["isDirectory"], false);

    // 轮询检测修改 → resource.changed（provider_watch 源）；等待 ≤3s。
    std::fs::write(&watched, b"v2-longer").expect("modify");
    let mut saw_change = false;
    for _ in 0..60 {
        let events = fx.io.events_since(0);
        if events["events"].as_array().unwrap().iter().any(|e| {
            e["type"] == "resource.changed"
                && e["resourceKey"] == key
                && e["source"] == "provider_watch"
        }) {
            saw_change = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(saw_change, "polling watcher emits resource.changed");

    // 删除 → resource.deleted。
    std::fs::remove_file(&watched).expect("remove");
    let mut saw_delete = false;
    for _ in 0..60 {
        let events = fx.io.events_since(0);
        if events["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "resource.deleted" && e["resourceKey"] == key)
        {
            saw_delete = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(saw_delete, "polling watcher emits resource.deleted");

    // 释放一个仍保活；两个都释放 → entry 消失（:199-209）。
    assert!(fx.io.unwatch(&w1));
    assert_eq!(fx.io.diagnostics()["watches"].as_array().unwrap().len(), 1);
    assert!(fx.io.unwatch(&w2));
    assert_eq!(fx.io.diagnostics()["watches"].as_array().unwrap().len(), 0);
    assert!(!fx.io.unwatch(&w1));

    // subscribe：UUID subscriptionId + resourceKeys（:96-127）；
    // unsubscribe 释放全部 retain。
    std::fs::write(&watched, b"v3").expect("rewrite");
    let sub = fx
        .io
        .subscribe(&serde_json::json!({
            "resources": [{"kind": "local-file", "path": watched.to_str().unwrap()}],
            "purpose": "test",
        }))
        .await
        .expect("subscribe");
    let sub_id = sub["subscriptionId"].as_str().expect("subscriptionId");
    assert_eq!(sub_id.len(), 36, "uuid v4 shape: {sub_id}");
    assert_eq!(sub["resourceKeys"], serde_json::json!([key]));
    assert_eq!(fx.io.diagnostics()["subscriptions"], 1);
    assert!(!fx.io.unsubscribe("00000000-0000-0000-0000-000000000000"));
    assert!(fx.io.unsubscribe(sub_id));
    assert_eq!(fx.io.diagnostics()["subscriptions"], 0);
    assert_eq!(fx.io.diagnostics()["watches"].as_array().unwrap().len(), 0);
    // 空资源 → 400（:102）。
    let err = fx.io.subscribe(&serde_json::json!({})).await.unwrap_err();
    assert_eq!(err.status, 400);
}

// ── 路由级（真 TCP + 真 HTTP，15 叶面） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-io-http-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-io-http-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    // macOS /var → /private/var：实现侧产出真实路径，夹具先规范化再断言。
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let deps = ServiceDeps {
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        home,
        workspace,
        token,
        stop: stop_tx,
        handle,
    }
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.workspace);
    }

    async fn post(&self, path: &str, body: &serde_json::Value) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(&body.to_string()),
        )
        .await
        .into_status_body()
    }

    async fn get(&self, path: &str) -> (u16, String) {
        http(
            &self.addr,
            "GET",
            path,
            &[("Authorization", &self.bearer())],
            None,
        )
        .await
        .into_status_body()
    }

    async fn delete(&self, path: &str) -> (u16, String) {
        http(
            &self.addr,
            "DELETE",
            path,
            &[("Authorization", &self.bearer())],
            None,
        )
        .await
        .into_status_body()
    }
}

struct HttpResponse {
    status: u16,
    #[allow(dead_code)] // 统一 HTTP 夹具形状；本套件只断言状态与体。
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn into_status_body(self) -> (u16, String) {
        (
            self.status,
            String::from_utf8_lossy(&self.body).into_owned(),
        )
    }
}

async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> HttpResponse {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| (p, p + 4));
    let (head_bytes, body_bytes) = match split {
        Some((h, b)) => (&raw[..h], &raw[b..]),
        None => (&raw[..], &[][..]),
    };
    let head = String::from_utf8_lossy(head_bytes).into_owned();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    HttpResponse {
        status,
        headers,
        body: body_bytes.to_vec(),
    }
}

/// 15 叶面全链：stat/read/list/search/write/write-expected-version/rename/
/// move/trash/subscribe/unsubscribe/watch/unwatch/watch-diagnostics/events。
#[tokio::test]
async fn resourceio_route_faces_and_error_vocabulary() {
    let server = start_server("faces").await;
    let target = server.workspace.join("route-note.txt");
    let target_ref = serde_json::json!({"kind": "local-file", "path": target.to_str().unwrap()});

    // ── stat ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/stat",
            &serde_json::json!({"resource": target_ref}),
        )
        .await;
    assert_eq!(status, 200, "stat: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["exists"],
        false
    );

    // ── write（utf-8 默认） ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write",
            &serde_json::json!({"resource": target_ref, "content": "hello 路由"}),
        )
        .await;
    assert_eq!(status, 200, "write: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["changeType"],
        "created"
    );

    // ── read（默认 utf-8 + base64 显式） ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/read",
            &serde_json::json!({"resource": target_ref}),
        )
        .await;
    assert_eq!(status, 200, "read: {body}");
    let read = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(read["content"], "hello 路由");
    assert_eq!(read["encoding"], "utf-8");
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/read",
            &serde_json::json!({"resource": target_ref, "encoding": "base64"}),
        )
        .await;
    assert_eq!(status, 200, "read b64: {body}");
    let read = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(read["encoding"], "base64");
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(read["content"].as_str().unwrap())
        .expect("base64 decode");
    assert_eq!(decoded, "hello 路由".as_bytes());

    // 二进制内容按 utf-8 读 → 400 invalid_resource_encoding（routes
    // :190-197）。
    let bin = server.workspace.join("bin.dat");
    std::fs::write(&bin, [0xff, 0xfe, 0x00, 0x01]).expect("write bin");
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/read",
            &serde_json::json!({"resource": {"kind": "local-file", "path": bin.to_str().unwrap()}}),
        )
        .await;
    assert_eq!(status, 400, "utf8 fatal: {body}");
    assert!(body.contains("invalid_resource_encoding"), "{body}");

    // 坏 base64 写入 → 400 invalid_resource_encoding（:199-208）。
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write",
            &serde_json::json!({"resource": target_ref, "content": "!!!", "encoding": "base64"}),
        )
        .await;
    assert_eq!(status, 400, "bad base64: {body}");
    assert!(body.contains("invalid_resource_encoding"), "{body}");

    // base64 写入真解码落盘（叶 #8/#9 份额）。
    let b64 = base64::engine::general_purpose::STANDARD.encode(b"\x01\x02\x03");
    let (status, _) = server
        .post(
            "/lingxi/v1/resource-io/write",
            &serde_json::json!({"resource": {"kind": "local-file", "path": bin.to_str().unwrap()}, "content": b64, "encoding": "base64"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(std::fs::read(&bin).expect("read bin"), b"\x01\x02\x03");

    // ── write-expected-version：冲突 → 409 + safeMessage（resourceJson
    // :216-221）──
    let stat = server
        .post(
            "/lingxi/v1/resource-io/stat",
            &serde_json::json!({"resource": target_ref}),
        )
        .await;
    let version = serde_json::from_str::<serde_json::Value>(&stat.1).unwrap()["version"].clone();
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write-expected-version",
            &serde_json::json!({"resource": target_ref, "content": "v2", "expectedVersion": version}),
        )
        .await;
    assert_eq!(status, 200, "wev ok: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write-expected-version",
            &serde_json::json!({"resource": target_ref, "content": "v3", "expectedVersion": version}),
        )
        .await;
    assert_eq!(status, 409, "wev conflict: {body}");
    let conflict = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(conflict["conflict"], true);
    assert_eq!(conflict["ok"], false);
    assert_eq!(conflict["safeMessage"], "Resource write conflict");
    assert_eq!(std::fs::read_to_string(&target).expect("read back"), "v2");

    // ── rename / move（跨 provider 拒绝走错误面） ──
    let renamed = server.workspace.join("route-renamed.txt");
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/rename",
            &serde_json::json!({
                "from": target_ref,
                "to": {"kind": "local-file", "path": renamed.to_str().unwrap()},
            }),
        )
        .await;
    assert_eq!(status, 200, "rename: {body}");
    assert!(renamed.exists() && !target.exists());
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/move",
            &serde_json::json!({
                "from": {"kind": "local-file", "path": renamed.to_str().unwrap()},
                "to": {"kind": "session-file", "fileId": "sf_any"},
            }),
        )
        .await;
    assert_eq!(status, 501, "cross-provider move: {body}");
    assert!(body.contains("cross_provider_move_unsupported"), "{body}");

    // ── trash ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/trash",
            &serde_json::json!({"resource": {"kind": "local-file", "path": renamed.to_str().unwrap()}}),
        )
        .await;
    assert_eq!(status, 200, "trash: {body}");
    let trashed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert!(trashed["trashId"].as_str().unwrap().starts_with("trash_"));
    assert!(!renamed.exists());

    // ── list / search ──
    // rename+trash 已把 route-note.txt 移走；list/search 断言前经写面重建
    //（内容与 search 的 query "v2" 对应）。
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write",
            &serde_json::json!({"resource": target_ref, "content": "v2"}),
        )
        .await;
    assert_eq!(status, 200, "rewrite for list: {body}");
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/list",
            &serde_json::json!({"resource": {"kind": "local-file", "path": server.workspace.to_str().unwrap()}}),
        )
        .await;
    assert_eq!(status, 200, "list: {body}");
    let names: Vec<String> = serde_json::from_str::<serde_json::Value>(&body).unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["bin.dat", "route-note.txt"]);
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/search",
            &serde_json::json!({"resource": {"kind": "local-file", "path": server.workspace.to_str().unwrap()}, "query": "v2"}),
        )
        .await;
    assert_eq!(status, 200, "search: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["matches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // ── subscribe / unsubscribe ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/subscribe",
            &serde_json::json!({"resource": target_ref, "purpose": "route-test"}),
        )
        .await;
    assert_eq!(status, 200, "subscribe: {body}");
    let sub = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(sub["ok"], true);
    let sub_id = sub["subscriptionId"].as_str().unwrap().to_string();
    let (status, body) = server
        .delete(&format!("/lingxi/v1/resource-io/subscriptions/{sub_id}"))
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["released"],
        true
    );
    let (_, body) = server
        .delete(&format!("/lingxi/v1/resource-io/subscriptions/{sub_id}"))
        .await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["released"],
        false
    );

    // ── watch / unwatch / watch-diagnostics ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/watch",
            &serde_json::json!({"resource": target_ref}),
        )
        .await;
    assert_eq!(status, 200, "watch: {body}");
    let watch_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["watchId"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, body) = server.get("/lingxi/v1/resource-io/watch-diagnostics").await;
    assert_eq!(status, 200, "diagnostics: {body}");
    let diag = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(diag["ok"], true);
    assert_eq!(diag["diagnostics"]["watches"].as_array().unwrap().len(), 1);
    let (status, body) = server
        .delete(&format!("/lingxi/v1/resource-io/watch/{watch_id}"))
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["released"],
        true
    );

    // ── events：写操作已 emit changed；since 严格化（D12） ──
    let (status, body) = server.get("/lingxi/v1/resource-io/events?since=0").await;
    assert_eq!(status, 200, "events: {body}");
    let events = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(events["stale"], false);
    assert!(events["latestSequence"].as_u64().unwrap() >= 1);
    assert!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "resource.changed"),
        "{body}"
    );
    let (status, _) = server.get("/lingxi/v1/resource-io/events?since=abc").await;
    assert_eq!(status, 400, "malformed cursor must be 400 (D12)");
    let (status, _) = server
        .get("/lingxi/v1/resource-io/events?since=0&alien=1")
        .await;
    assert_eq!(status, 400, "unknown query key must be 400 (D12)");
    let (status, _) = server
        .get("/lingxi/v1/resource-io/events?since=0&since=1")
        .await;
    assert_eq!(status, 400, "duplicate since must be 400 (D12)");

    // ── mount 引用显式拒绝（D9/J2） ──
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/stat",
            &serde_json::json!({"resource": {"kind": "mount", "mountId": "m1", "path": "a"}}),
        )
        .await;
    assert_eq!(status, 400, "mount refused: {body}");
    assert!(body.contains("unsupported_provider"), "{body}");

    // ── LocalOnly：无凭证 → 401；未注册形状 → 404（fail-closed 到底） ──
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/resource-io/stat",
        &[],
        Some("{}"),
    )
    .await;
    assert_eq!(resp.status, 401);
    let (status, _) = server
        .post("/lingxi/v1/resource-io/alien-op", &serde_json::json!({}))
        .await;
    assert_eq!(status, 404);

    server.stop_and_clean().await;
}

/// 无 workspace 时 local_fs provider 缺席：本地引用 → 501
/// provider_not_available（现役 provider 缺失词汇，errors.ts :20-25），
/// session_file 面照常可用。
#[tokio::test]
async fn route_without_workspace_keeps_session_file_face() {
    let uniq = format!(
        "{}-{}-noworkspace",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-io-http-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = ready_rx.await.expect("readiness");
    let bearer = format!("Bearer {token}");
    let resp = http(
        &addr,
        "POST",
        "/lingxi/v1/resource-io/stat",
        &[("Authorization", &bearer)],
        Some(
            &serde_json::json!({"resource": {"kind": "local-file", "path": "/tmp/x"}}).to_string(),
        ),
    )
    .await;
    assert_eq!(resp.status, 501, "no local_fs without workspace");
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    assert!(body.contains("provider_not_available"), "{body}");

    stop_tx.send(()).expect("stop");
    tokio::time::timeout(Duration::from_secs(10), handle)
        .await
        .expect("shutdown")
        .expect("join")
        .expect("serve outcome");
    let _ = std::fs::remove_dir_all(&home);
}
