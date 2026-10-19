use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_default_limit_with_more_than_eighty_real_matches() {
    let h = Harness::boot("search-limit-proof").await;
    for index in 0..101 {
        std::fs::write(
            h.workspace.join(format!("bulk-match-{index:03}.txt")),
            "entry",
        )
        .unwrap();
    }
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/search?q=bulk-match-",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 80, "{body}");
    h.shutdown().await;
}

/// 同一正式处理器在非 default 挂载上的实际动作，不用模型化文件系统替身。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nondefault_mount_actions_upload_and_content_change_real_files() {
    let h = Harness::boot("mount-core-proof").await;
    let mounted = h.home.join("mounted-proof");
    std::fs::create_dir(&mounted).unwrap();
    let (status, result) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({"path":mounted})),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let mount_id = result["workspace"]["mountId"].as_str().unwrap().to_string();

    let (status, result) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action":"mkdir", "mountId":mount_id, "name":"alpha"
            })),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert!(mounted.join("alpha").is_dir());

    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"create", "mountId":mount_id, "subdir":"alpha", "name":"one.txt", "content":"first"
    }))).await;
    assert_eq!(status, 200, "{result}");
    let version = result["version"].clone();
    assert_eq!(
        std::fs::read_to_string(mounted.join("alpha/one.txt")).unwrap(),
        "first"
    );
    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"writeText", "mountId":mount_id, "subdir":"alpha", "name":"one.txt", "content":"second", "expectedVersion":version
    }))).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ok"], true);
    assert_eq!(
        std::fs::read_to_string(mounted.join("alpha/one.txt")).unwrap(),
        "second"
    );
    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"writeText", "mountId":mount_id, "subdir":"alpha", "name":"one.txt", "content":"must not replace", "expectedVersion":version
    }))).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["conflict"], true);
    assert_eq!(
        std::fs::read_to_string(mounted.join("alpha/one.txt")).unwrap(),
        "second"
    );

    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"rename", "mountId":mount_id, "subdir":"alpha", "oldName":"one.txt", "newName":"renamed.txt"
    }))).await;
    assert_eq!(status, 200, "{result}");
    assert!(!mounted.join("alpha/one.txt").exists());
    assert_eq!(
        std::fs::read_to_string(mounted.join("alpha/renamed.txt")).unwrap(),
        "second"
    );

    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"move", "mountId":mount_id, "subdir":"alpha", "name":"renamed.txt", "destSubdir":"beta"
    }))).await;
    assert_eq!(status, 200, "{result}");
    assert!(!mounted.join("alpha/renamed.txt").exists());
    assert_eq!(
        std::fs::read_to_string(mounted.join("beta/renamed.txt")).unwrap(),
        "second"
    );

    let (status, result) = h.http(reqwest::Method::POST, "/lingxi/v1/workbench/actions", Some(json!({
        "action":"movePaths", "mountId":mount_id, "items":[{"sourceSubdir":"beta","name":"renamed.txt"}],
        "destSubdir":"alpha", "currentSubdir":"beta"
    }))).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["results"][0]["ok"], true);
    assert!(result["filesByPath"]["alpha"].is_array());
    assert!(!mounted.join("beta/renamed.txt").exists());
    assert_eq!(
        std::fs::read_to_string(mounted.join("alpha/renamed.txt")).unwrap(),
        "second"
    );

    let (status, result) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/upload",
            Some(json!({
                "mountId":mount_id,"subdir":"alpha","files":[
                    {"name":"blob.bin","contentBase64":b64(b"mounted upload")},
                    {"name":"../escape.bin","contentBase64":b64(b"must not escape")}
                ]
            })),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ok"], false);
    assert_eq!(result["results"][0]["ok"], true);
    assert_eq!(result["results"][1]["ok"], false);
    assert_eq!(
        std::fs::read(mounted.join("alpha/blob.bin")).unwrap(),
        b"mounted upload"
    );
    assert!(!mounted.join("escape.bin").exists());

    let content_path =
        format!("/lingxi/v1/workbench/content?mountId={mount_id}&subdir=alpha&name=blob.bin");
    let (status, _, bytes) = h.http_raw(reqwest::Method::GET, &content_path, &[]).await;
    assert_eq!(status, 200);
    assert_eq!(bytes, b"mounted upload");
    let (status, headers, bytes) = h.http_raw(reqwest::Method::HEAD, &content_path, &[]).await;
    assert_eq!(status, 200);
    assert!(bytes.is_empty());
    assert_eq!(
        headers.get("content-length").unwrap().to_str().unwrap(),
        "14"
    );
    let (status, result) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/workbench/search?mountId={mount_id}&q=blob"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["results"][0]["name"], "blob.bin");

    let (status, result) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action":"safeDelete", "mountId":mount_id, "subdir":"alpha", "name":"renamed.txt"
            })),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let trash_id = result["trashId"].as_str().unwrap();
    assert!(!mounted.join("alpha/renamed.txt").exists());
    let metadata: Value = serde_json::from_slice(
        &std::fs::read(
            h.home
                .join("trash/mobile-workbench")
                .join(trash_id)
                .join("metadata.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["mountId"], mount_id);
    assert_eq!(metadata["originalSubdir"], "alpha");

    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/studio/workspaces/{mount_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let (status, _, _) = h.http_raw(reqwest::Method::GET, &content_path, &[]).await;
    assert_eq!(status, 404);
    assert_eq!(
        std::fs::read(mounted.join("alpha/blob.bin")).unwrap(),
        b"mounted upload"
    );
    h.shutdown().await;
}
