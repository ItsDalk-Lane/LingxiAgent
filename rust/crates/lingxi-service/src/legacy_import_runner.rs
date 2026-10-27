//! R08-T05（裁决点 P3，Lead 批准）：legacy-import 动词体从 main.rs 原样提取。
//!
//! 零行为变化契约：legacy-import 动词的 stderr 文案、退出码、stdout 形状
//! 逐字节保持；失败行以「完整行」形式随 VerbFailure 返回，调用方原样输出。
//! migrate 动词（切割协调器，R08-T05）与本模块共用同一条实现链——协调器的
//! import 相 = acquire_target_lock + run_import，收据/簿记语义不变。

use lingxi_adapters::storage::legacy_import::{EngineOutcome, ImportArgs};

/// 目标根布局 + 单写者锁守卫。两字段都不被读取：guard 的价值是 Drop
/// 语义（存活即持锁，析构即释放），调用方须把整个 TargetLock 绑定到
/// 动词全程的作用域。
pub struct TargetLock {
    #[allow(dead_code)]
    pub layout: lingxi_service::paths::DataRootLayout,
    #[allow(dead_code)]
    pub guard: lingxi_service::instance::InstanceGuard,
}

/// 目标根准备（按需创建 → canonicalize → prepare_layout）+ 实例互斥锁获取。
/// 失败返回（退出码, 完整 stderr 行），与提取前逐字节一致：
/// 准备失败 code 2；锁被活实例/导入器持有 code 3。
pub fn acquire_target_lock(target: &std::path::Path) -> Result<TargetLock, (u8, String)> {
    // 实例互斥（批准设计⑤）：导入全程持有目标根单写者锁——同一目标根
    // 同时只有一个 importer/服务；也是 kill -9 复核路径「running 但无
    // 活锁持有者 → 判 interrupted」的裁决依据。持锁失败响亮退出。
    if !target.exists() {
        if let Err(e) = std::fs::create_dir_all(target) {
            return Err((
                2,
                format!(
                    "error: legacy-import: cannot create target {}: {e}",
                    target.display()
                ),
            ));
        }
    }
    let target_abs = match std::fs::canonicalize(target) {
        Ok(p) => p,
        Err(e) => {
            return Err((
                2,
                format!(
                    "error: legacy-import: cannot canonicalize target {}: {e}",
                    target.display()
                ),
            ));
        }
    };
    let layout = match lingxi_service::paths::prepare_layout(&target_abs) {
        Ok(layout) => layout,
        Err(e) => {
            return Err((2, format!("error: legacy-import: target layout: {e}")));
        }
    };
    let guard = match lingxi_service::instance::acquire(&layout) {
        Ok((guard, _stale)) => guard,
        Err(e) => {
            return Err((
                3,
                format!(
                    "error: legacy-import: target {} is locked by another live instance/importer: {e}",
                    target_abs.display()
                ),
            ));
        }
    };
    Ok(TargetLock { layout, guard })
}

/// 动词失败：code = 进程退出码；line = 完整 stderr 行（含前缀），原样输出。
#[derive(Debug)]
pub struct VerbFailure {
    pub code: u8,
    pub line: String,
}

/// 导入实现链：adapters 引擎（专用 OS 线程）→ 中断即返（等 --resume）→
/// session-files → trace → runtime-state → cron → surfaces 五个收尾阶段。
/// 任一阶段失败 = VerbFailure（响亮 exit 2），绝不静默吞错。
pub async fn run_import(args: ImportArgs) -> Result<EngineOutcome, VerbFailure> {
    // A07 附件阶段还需要一份 args（线程拿走原值）。
    let args_for_files = ImportArgs {
        source: args.source.clone(),
        target: args.target.clone(),
        resume: args.resume.clone(),
        stop_after_batch: args.stop_after_batch,
        instance_lock_held: true,
    };
    // 导入器自带 current_thread runtime 打开 canonical 存储；若在 main 的
    // tokio runtime 里直接 block_on 会 panic。离线动词 → 专用 OS 线程执行。
    let outcome = std::thread::Builder::new()
        .name("r08-legacy-import".into())
        .spawn(move || lingxi_adapters::storage::legacy_import::run(&args))
        .map_err(|e| format!("spawn import thread: {e}"))
        .and_then(|h| h.join().map_err(|_| "import thread panicked".to_string()));
    match outcome {
        Ok(Ok(mut outcome)) => {
            if outcome.interrupted {
                // A08：--stop-after-batch 批次边界干净停下——跳过后续所有
                // 阶段（session-files/trace/runtime-state/cron/surfaces），
                // 等 --resume 续跑。收据已是 partial，落账即真相。
                return Ok(outcome);
            }
            // A07 附件实转（B2 切片A）：session-files 目录级 convert 批次
            // 由 service 侧 sessionfiles 扩展执行（canonical 写路径唯一），
            // adapters 侧只负责落账 + 收据重写。失败响亮：error → exit 2。
            match legacy_import_session_files_phase(&args_for_files, &outcome).await {
                Ok(Some(final_outcome)) => outcome = final_outcome,
                Ok(None) => {}
                Err(err) => {
                    return Err(VerbFailure {
                        code: 2,
                        line: format!("error: legacy-import session-files phase: {err}"),
                    });
                }
            }
            // B3 切片 B：旧 trace 导入（observability sqlite →
            // model_call_usage，同 call ID 幂等）。失败响亮：error → exit 2。
            match legacy_import_trace_phase(&args_for_files, &outcome).await {
                Ok(Some(final_outcome)) => outcome = final_outcome,
                Ok(None) => {}
                Err(err) => {
                    return Err(VerbFailure {
                        code: 2,
                        line: format!("error: legacy-import trace phase: {err}"),
                    });
                }
            }
            // B3 切片 C：8 个运行态类逐字节归档 + V28 键登记（同步小 IO）。
            match lingxi_adapters::storage::legacy_import::convert_runtime_state_phase(
                &args_for_files,
                &outcome.run_id,
                outcome.resumed,
            ) {
                Ok(Some(final_outcome)) => outcome = final_outcome,
                Ok(None) => {}
                Err(err) => {
                    return Err(VerbFailure {
                        code: 2,
                        line: format!("error: legacy-import runtime-state phase: {err}"),
                    });
                }
            }
            // B3 切片 B（cron 半）：旧自动化任务导入（import_current_studio
            // 同一实现链，同 job 幂等）。失败响亮：error → exit 2。
            match legacy_import_cron_phase(&args_for_files, &outcome).await {
                Ok(Some(final_outcome)) => outcome = final_outcome,
                Ok(None) => {}
                Err(err) => {
                    return Err(VerbFailure {
                        code: 2,
                        line: format!("error: legacy-import cron phase: {err}"),
                    });
                }
            }
            // B3 切片 D：plugins/epoch/knowledge 三个面 + stamp=1。
            match legacy_import_surfaces_phase(&args_for_files, &outcome) {
                Ok(Some(final_outcome)) => outcome = final_outcome,
                Ok(None) => {}
                Err(err) => {
                    return Err(VerbFailure {
                        code: 2,
                        line: format!("error: legacy-import surfaces phase: {err}"),
                    });
                }
            }
            Ok(outcome)
        }
        Ok(Err(err)) => Err(VerbFailure {
            code: 2,
            line: format!("error: legacy-import: {err}"),
        }),
        Err(err) => Err(VerbFailure {
            code: 2,
            line: format!("error: legacy-import: {err}"),
        }),
    }
}

/// 动词 stdout 形状（逐字节保持提取前语义）：
/// 中断 = pretty JSON；完成 = 紧凑 JSON（序列化失败时退化为最小行）。
pub fn print_outcome(outcome: &EngineOutcome) {
    if outcome.interrupted {
        println!(
            "{}",
            serde_json::to_string_pretty(outcome).unwrap_or_default()
        );
        return;
    }
    match serde_json::to_string(outcome) {
        Ok(line) => println!("{line}"),
        Err(_) => println!(
            "{{\"runId\":\"{}\",\"status\":\"{}\"}}",
            outcome.run_id, outcome.status
        ),
    }
}

/// R08-T04 A07：session-files 目录级批次实转阶段（B2 切片A）。
/// adapters::run 完成其余全部批次后调用；canonical 写路径唯一（service 侧
/// sessionfiles 扩展），落账与收据重写交回 adapters。无待执行目录 → None。
async fn legacy_import_session_files_phase(
    args: &ImportArgs,
    outcome: &EngineOutcome,
) -> Result<Option<EngineOutcome>, String> {
    use lingxi_adapters::storage::legacy_import as li;
    let pending =
        li::pending_session_files_dirs(&args.target, &outcome.run_id).map_err(|e| e.to_string())?;
    if pending.is_empty() {
        return Ok(None);
    }
    let db_path = args.target.join(li::TARGET_DB_RELPATH);
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let service = lingxi_service::sessionfiles::SessionFileService::new(
        args.target.clone(),
        std::sync::Arc::new(db),
        || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        },
    );
    let staging_dir = args.target.join(li::RECEIPT_DIR_RELPATH).join("staging");
    let mut outcomes = Vec::new();
    for (batch_key, dir_rel) in pending {
        let source_dir = args.source.join(&dir_rel);
        match service
            .import_legacy_session_files_dir(&source_dir, &staging_dir)
            .await
        {
            Ok(report) => outcomes.push(li::SessionFilesDirOutcome {
                batch_key,
                imported: report.imported,
                skipped_existing: report.skipped_existing,
                broken: report
                    .broken
                    .into_iter()
                    .map(|b| (b.file_id, b.reason))
                    .collect(),
                error: None,
            }),
            Err(err) => outcomes.push(li::SessionFilesDirOutcome {
                batch_key,
                imported: 0,
                skipped_existing: 0,
                broken: Vec::new(),
                error: Some(err.to_string()),
            }),
        }
    }
    let final_outcome =
        li::finalize_session_files(args, &outcome.run_id, outcome.resumed, &outcomes)
            .map_err(|e| e.to_string())?;
    Ok(Some(final_outcome))
}

/// B3 切片 B：旧 trace 导入阶段。register done 的 model-observability 主库
/// → adapters 只读映射（与端点同一实现链）→ RunDatabase::import_legacy_model_calls
/// （同 call ID 幂等、冲突拒）。结果记入 V28 扩展收据 legacyTrace；单库失败
/// → error 级 issue + 该库 status=failed（其余库继续），绝不静默吞错。
async fn legacy_import_trace_phase(
    args: &ImportArgs,
    outcome: &EngineOutcome,
) -> Result<Option<EngineOutcome>, String> {
    use lingxi_adapters::storage::legacy_import as li;
    let pending = li::pending_legacy_trace_stores(&args.target, &outcome.run_id)
        .map_err(|e| e.to_string())?;
    if pending.is_empty() {
        return Ok(None);
    }
    let db_path = args.target.join(li::TARGET_DB_RELPATH);
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut stores = Vec::new();
    let mut any_failed = false;
    for (batch_key, relpath) in pending {
        let source_path = args.source.join(&relpath);
        let source_sha = sha256_file(&source_path)?;
        match li::observability::read_legacy_observability_rows(&source_path) {
            Ok(rows) => {
                let trace_rows = rows.iter().filter(|r| r.legacy_trace_id.is_some()).count();
                match db.import_legacy_model_calls(rows, now_ms).await {
                    Ok(stats) => stores.push(serde_json::json!({
                        "sourceRelpath": relpath,
                        "sourceSha256": source_sha,
                        "status": "imported",
                        "importedCalls": stats.inserted,
                        "skippedExisting": stats.skipped_existing,
                        "traceRows": trace_rows,
                    })),
                    Err(err) => {
                        any_failed = true;
                        let msg = format!("legacy trace import refused by write path: {err}");
                        li::record_issue(
                            &args.target,
                            &outcome.run_id,
                            &batch_key,
                            "error",
                            "unmapped-shape",
                            Some(&relpath),
                            &msg,
                        )
                        .map_err(|e| e.to_string())?;
                        stores.push(serde_json::json!({
                            "sourceRelpath": relpath,
                            "sourceSha256": source_sha,
                            "status": "failed",
                            "error": msg,
                        }));
                    }
                }
            }
            Err(err) => {
                any_failed = true;
                let msg = format!("legacy observability store unreadable: {err}");
                li::record_issue(
                    &args.target,
                    &outcome.run_id,
                    &batch_key,
                    "error",
                    "unmapped-shape",
                    Some(&relpath),
                    &msg,
                )
                .map_err(|e| e.to_string())?;
                stores.push(serde_json::json!({
                    "sourceRelpath": relpath,
                    "sourceSha256": source_sha,
                    "status": "failed",
                    "error": msg,
                }));
            }
        }
    }
    let imported_total: u64 = stores
        .iter()
        .filter_map(|s| s.get("importedCalls").and_then(|v| v.as_u64()))
        .sum();
    let skipped_total: u64 = stores
        .iter()
        .filter_map(|s| s.get("skippedExisting").and_then(|v| v.as_u64()))
        .sum();
    let trace_total: u64 = stores
        .iter()
        .filter_map(|s| s.get("traceRows").and_then(|v| v.as_u64()))
        .sum();
    li::record_extension(
        &args.target,
        &outcome.run_id,
        "legacyTrace",
        &serde_json::json!({
            "stores": stores,
            "importedCalls": imported_total,
            "skippedExisting": skipped_total,
            "traceRows": trace_total,
            "failedStores": any_failed,
        }),
    )
    .map_err(|e| e.to_string())?;
    let final_outcome = li::finalize_extensions(args, &outcome.run_id, outcome.resumed)
        .map_err(|e| e.to_string())?;
    Ok(Some(final_outcome))
}

/// B3 切片 B（cron 半）：cron-automation register 批次存在时，复用
/// import_current_studio 与现役自动化导入同一条链（MemoryService 绑定源
/// home，零写副作用已验证；storage 指向目标 runs.db）。Blocked 逐条进
/// issues（响亮），结果记入扩展收据 legacyCron。
async fn legacy_import_cron_phase(
    args: &ImportArgs,
    outcome: &EngineOutcome,
) -> Result<Option<EngineOutcome>, String> {
    use lingxi_adapters::storage::legacy_import as li;
    let has = li::has_register_done_batches(&args.target, &outcome.run_id, "cron-automation")
        .map_err(|e| e.to_string())?;
    if !has {
        return Ok(None);
    }
    let db_path = args.target.join(li::TARGET_DB_RELPATH);
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let api = lingxi_service::automation::JobApi {
        storage: std::sync::Arc::new(db),
        memory: std::sync::Arc::new(lingxi_service::memory::MemoryService::new(
            args.source.clone(),
            args.source.join(".lingxi-runtime-readonly"),
        )),
        clock: std::sync::Arc::new(lingxi_service::inject::SystemClock),
        request_ids: std::sync::Arc::new(lingxi_service::inject::RandomRequestIdGen),
        studio_id: "legacy-import".to_string(),
    };
    let report = lingxi_service::automation_import::import_current_studio(&api).await;
    // Blocked 逐条响亮落 issues；不静默吞任何一条。
    for entry in &report.entries {
        if matches!(
            entry.status,
            lingxi_service::automation_import::ImportStatus::Blocked
        ) {
            li::record_issue(
                &args.target,
                &outcome.run_id,
                "cron-automation/import",
                "error",
                "unmapped-shape",
                entry.legacy_job_id.as_deref(),
                &format!(
                    "legacy cron job blocked: {}",
                    entry.detail.as_deref().unwrap_or("no detail")
                ),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    li::record_extension(
        &args.target,
        &outcome.run_id,
        "legacyCron",
        &serde_json::json!({
            "imported": report.imported,
            "alreadyPresent": report.already_present,
            "blocked": report.blocked,
            "entries": report.entries,
        }),
    )
    .map_err(|e| e.to_string())?;
    let final_outcome = li::finalize_extensions(args, &outcome.run_id, outcome.resumed)
        .map_err(|e| e.to_string())?;
    Ok(Some(final_outcome))
}

/// B3 切片 D：plugins / epoch / knowledge 三个面。
/// - 旧插件：warn_if_legacy_plugins 同一探测链，检出逐名 info issue
///   （legacy-plugins-detected）+ 扩展收据 legacyPlugins；
/// - epoch：目标根落 stamp=1（write_legacy_import_stamp，启动门同一实现链）；
/// - knowledge.db：存在即取证（sha256 + 每表行数）进扩展收据 knowledgeDb，
///   不存在如实记录 present=false。全部失败响亮。
fn legacy_import_surfaces_phase(
    args: &ImportArgs,
    outcome: &EngineOutcome,
) -> Result<Option<EngineOutcome>, String> {
    use lingxi_adapters::storage::legacy_import as li;

    // plugins 面
    let detected = lingxi_service::legacy_plugins::warn_if_legacy_plugins(&args.source);
    for name in &detected {
        li::record_issue(
            &args.target,
            &outcome.run_id,
            "legacy-plugins/detected",
            "info",
            "legacy-plugins-detected",
            Some(name),
            &format!("legacy user plugin {name} is not loaded; surface recorded only"),
        )
        .map_err(|e| e.to_string())?;
    }
    li::record_extension(
        &args.target,
        &outcome.run_id,
        "legacyPlugins",
        &serde_json::json!({
            "detected": detected,
            "count": detected.len(),
            "note": "旧用户插件不加载；仅识别与取证",
        }),
    )
    .map_err(|e| e.to_string())?;

    // epoch 面：目标根 stamp=1
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as i64;
    lingxi_service::epoch::write_legacy_import_stamp(&args.target, now_ms)?;
    li::record_extension(
        &args.target,
        &outcome.run_id,
        "epoch",
        &serde_json::json!({"stamp": 1, "written": true}),
    )
    .map_err(|e| e.to_string())?;

    // knowledge.db 面：存在即取证，不存在如实记录
    let knowledge_path = args.source.join("knowledge.db");
    let payload = if knowledge_path.exists() {
        let sha256 = sha256_file(&knowledge_path)?;
        let conn = rusqlite::Connection::open_with_flags(
            &knowledge_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("cannot open legacy knowledge.db read-only: {e}"))?;
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table'
                 AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(|e| format!("cannot inspect legacy knowledge.db schema: {e}"))?;
        let tables = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("cannot inspect legacy knowledge.db schema: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("cannot inspect legacy knowledge.db schema: {e}"))?;
        let mut table_rows = Vec::new();
        for table in tables {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                    row.get(0)
                })
                .map_err(|e| format!("cannot count legacy knowledge.db rows: {e}"))?;
            table_rows.push(serde_json::json!({"table": table, "rows": count}));
        }
        serde_json::json!({
            "present": true,
            "sourceRelpath": "knowledge.db",
            "sourceSha256": sha256,
            "tables": table_rows,
            "note": "取证快照；消费端待对应任务",
        })
    } else {
        serde_json::json!({"present": false, "note": "源侧无 knowledge.db"})
    };
    li::record_extension(&args.target, &outcome.run_id, "knowledgeDb", &payload)
        .map_err(|e| e.to_string())?;

    let final_outcome = li::finalize_extensions(args, &outcome.run_id, outcome.resumed)
        .map_err(|e| e.to_string())?;
    Ok(Some(final_outcome))
}

/// 阶段用源文件 sha256（与清单同一算法与 hex 口径，字节全读）。
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::Digest as _;
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let digest = sha2::Sha256::digest(&bytes);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}
