//! 原巡检约定：材料来自资源服务，计划不能读取工作区或替执行结果签字。
use chrono::{FixedOffset, TimeZone};
use serde_json::json;
use std::path::PathBuf;

// 独立编译计划模块，组合根接线前也能验证规则；不绕过真实资源读写验收。
#[allow(dead_code)]
#[path = "../src/automation_heartbeat.rs"]
mod automation_heartbeat;
use automation_heartbeat::*;
use lingxi_test_guard as _;

fn file(name: &str, is_directory: bool, modified_ms: u64) -> HeartbeatFile {
    HeartbeatFile {
        name: name.into(),
        is_directory,
        size: 12,
        modified_ms,
    }
}

fn materials() -> HeartbeatMaterials {
    HeartbeatMaterials {
        workspace: PathBuf::from(if cfg!(windows) {
            "C:\\synthetic\\desk"
        } else {
            "/synthetic/desk"
        }),
        desk_files: vec![
            file("report.txt", false, 10),
            file("project", true, 20),
            file("OH-Works", true, 30),
            file(".secret", false, 40),
        ],
        overwatch: Some("检查待办事项".into()),
        patrol_log: Some("- [2026-10-15 09:00] 已检查报告\n".into()),
        jian_dirs: vec![
            JianDirectory {
                relative_dir: ".".into(),
                content: "执行五次\n".into(),
                files: vec![file("report.txt", false, 10), file("OH-Works", true, 30)],
            },
            JianDirectory {
                relative_dir: "project".into(),
                content: "检查项目\n".into(),
                files: vec![file("input.txt", false, 10)],
            },
        ],
    }
}

#[test]
fn auto_heartbeat_requires_explicit_agent_opt_in_and_master_gate() {
    assert!(
        !HeartbeatSettings::from_config(&json!({"desk":{}}), &json!({}))
            .unwrap()
            .enabled
    );
    assert!(
        HeartbeatSettings::from_config(&json!({"desk":{"heartbeat_enabled":true}}), &json!({}))
            .unwrap()
            .enabled
    );
    assert!(
        !HeartbeatSettings::from_config(
            &json!({"desk":{"heartbeat_enabled":true}}),
            &json!({"heartbeat_master":false})
        )
        .unwrap()
        .enabled
    );
    assert!(
        !HeartbeatSettings::from_config(&json!({"desk":{"heartbeat_enabled":"true"}}), &json!({}))
            .unwrap()
            .enabled
    );
    assert_eq!(
        HeartbeatSettings::from_config(&json!({"desk":{"heartbeat_interval":0}}), &json!({}))
            .unwrap()
            .interval_ms,
        DEFAULT_HEARTBEAT_INTERVAL_MS
    );
    assert_eq!(
        HeartbeatSettings::from_config(&json!({"desk":{"heartbeat_interval":1.5}}), &json!({}))
            .unwrap()
            .interval_ms,
        90_000
    );
    assert!(
        HeartbeatSettings::from_config(&json!({"desk":{"heartbeat_interval":-1}}), &json!({}))
            .is_err()
    );
    assert!(HeartbeatSettings::from_config(
        &json!({"desk":{"heartbeat_interval":"31"}}),
        &json!({})
    )
    .is_err());
}

#[test]
fn slots_are_unix_aligned_and_manual_requests_have_independent_cooldown() {
    let interval = DEFAULT_HEARTBEAT_INTERVAL_MS;
    assert_eq!(next_heartbeat_slot_ms(0, interval).unwrap(), interval);
    assert_eq!(
        next_heartbeat_slot_ms(interval, interval).unwrap(),
        interval * 2
    );
    assert_eq!(
        next_heartbeat_slot_ms(interval + 1, interval).unwrap(),
        interval * 2
    );
    assert!(next_heartbeat_slot_ms(0, 0).is_err());
    assert!(next_heartbeat_slot_ms(u64::MAX, interval).is_err());
    assert_eq!(manual_heartbeat_delay_ms(0, None), 0);
    assert_eq!(manual_heartbeat_delay_ms(120_000, Some(0)), 0);
    assert_eq!(manual_heartbeat_delay_ms(119_999, Some(0)), 1);
    assert_eq!(
        manual_heartbeat_delay_ms(100, Some(1_000)),
        MANUAL_HEARTBEAT_COOLDOWN_MS
    );
}

#[test]
fn real_resource_listing_shape_is_strict_and_no_bad_entry_becomes_an_empty_scan() {
    assert_eq!(files_from_resource_listing(&json!({"items":[{"name":"project","isDirectory":true,"size":null,"mtimeMs":4},{"name":"report.txt","isDirectory":false,"size":12,"mtimeMs":5}]})).unwrap(),vec![file("project",true,4),file("report.txt",false,5)].into_iter().map(|mut f| {if f.is_directory {f.size=0;} f}).collect::<Vec<_>>());
    for listing in [
        json!({"error":"denied"}),
        json!({"items":[{"name":"../outside","isDirectory":false,"size":1,"mtimeMs":0}]}),
        json!({"items":[{"name":"x","isDirectory":false,"size":1}]}),
        json!({"items":[{"name":"x","isDirectory":true,"mtimeMs":1},{"name":"x","isDirectory":true,"mtimeMs":1}]}),
    ] {
        assert!(files_from_resource_listing(&listing).is_err());
    }
}

#[test]
fn patrol_plan_excludes_own_outputs_and_includes_both_root_and_first_level_jian() {
    let input = materials();
    let mut previous = HeartbeatSnapshot::default();
    previous.desk_files.insert("removed.txt".into(), 0);
    previous.desk_files.insert("report.txt".into(), 1);
    let plan =
        build_heartbeat_plan("小/花:*?", "zh-CN", "2026-10-15 12:00", &input, &previous).unwrap();
    assert_eq!(plan.patrol.kind, HeartbeatKind::Patrol);
    assert!(plan.patrol.prompt.contains("不是用户发来的"));
    assert!(plan.patrol.prompt.contains("~ report.txt"));
    assert!(plan.patrol.prompt.contains("+ project"));
    assert!(plan.patrol.prompt.contains("- removed.txt"));
    assert!(plan.patrol.prompt.contains("近期巡检记录"));
    assert!(plan.patrol.prompt.contains("检查待办事项"));
    assert!(plan.patrol.prompt.contains("OH-Works/小花-activity/"));
    assert!(plan
        .patrol
        .log_target
        .path()
        .ends_with("OH-Works/小花的巡检/patrol-log.md"));
    assert_eq!(plan.desk_snapshot.len(), 2);
    assert!(!plan.desk_snapshot.contains_key("OH-Works"));
    assert!(!plan.desk_snapshot.contains_key(".secret"));
    assert_eq!(plan.jian.len(), 2);
    assert_eq!(plan.jian[0].cwd, input.workspace);
    assert_eq!(plan.jian[1].cwd, input.workspace.join("project"));
    assert!(plan
        .jian
        .iter()
        .all(|run| run.log_target.tool_name() == "jian_update_status"));
    assert!(!plan.jian[0].prompt.contains("📁 OH-Works"));
    assert!(!plan.jian[0].prompt.contains("📄 jian.md"));
}

#[test]
fn unchanged_jian_is_still_planned_so_model_can_continue_or_skip_using_saved_state() {
    let input = materials();
    let mut previous = HeartbeatSnapshot::default();
    previous.desk_files.insert("report.txt".into(), 10);
    previous.desk_files.insert("project".into(), 20);
    for directory in &input.jian_dirs {
        previous.jian.insert(
            directory.relative_dir.clone(),
            jian_fingerprint(
                &directory.content,
                &directory.files,
                directory.relative_dir == ".",
            )
            .unwrap(),
        );
    }
    let plan = build_heartbeat_plan("Hana", "en", "12:00", &input, &previous).unwrap();
    assert!(plan.patrol.prompt.contains("no file changes"));
    assert_eq!(plan.jian.len(), 2);
    assert!(plan.jian[0].prompt.contains("jian.md: unchanged"));
    assert!(plan.jian[0]
        .prompt
        .contains("do not call any tools and do not rewrite jian.md"));
    assert!(plan.jian[0].prompt.contains("4/5"));
    assert!(plan.jian[0].prompt.contains("risk level"));
    assert_eq!(previous.jian.len(), 2);
}

#[test]
fn deeper_hidden_unknown_and_duplicate_jian_roots_are_refused() {
    for relative in [
        "project/sub",
        "../outside",
        ".hidden",
        "OH-Works",
        "unknown",
        "project\\sub",
    ] {
        let mut input = materials();
        input.jian_dirs[1].relative_dir = relative.into();
        assert!(
            build_heartbeat_plan("Hana", "en", "12:00", &input, &HeartbeatSnapshot::default())
                .is_err(),
            "{relative}"
        );
    }
    let mut input = materials();
    input.jian_dirs.push(input.jian_dirs[0].clone());
    assert!(
        build_heartbeat_plan("Hana", "en", "12:00", &input, &HeartbeatSnapshot::default()).is_err()
    );
}

#[test]
fn jian_status_keeps_concurrent_user_body_and_program_owned_start_snapshot() {
    let mut input = materials();
    input.jian_dirs[0].content = "用户原任务 ```不要结束围栏```\n".into();
    let plan =
        build_heartbeat_plan("Hana", "zh", "12:00", &input, &HeartbeatSnapshot::default()).unwrap();
    let target = &plan.jian[0].log_target;
    let current = "用户刚改为执行三次\n\n<!-- exec-log -->\n旧状态\n<!-- /exec-log -->\n";
    let write = target
        .prepare_write(
            Some(current),
            &json!({"status":"in_progress","progress":"1/3","note":"完成第一次"}),
            "12:00",
        )
        .unwrap();
    let text = String::from_utf8(write.content).unwrap();
    assert!(text.starts_with("用户刚改为执行三次\n\n<!-- exec-log -->"));
    assert!(text.contains("````jian-snapshot"));
    assert!(text.contains("用户原任务 ```不要结束围栏```"));
    assert!(text.contains("- 进度：1/3"));
    assert!(!text.contains("旧状态"));
    assert_eq!(write.path, input.workspace.join("jian.md"));
    assert_eq!(write.capability, "jian_update_status.update");
    assert!(target
        .prepare_write(None, &json!({"status":"completed","note":"done"}), "12:00")
        .is_err());
    assert!(target
        .prepare_write(
            Some(current),
            &json!({"status":"invalid","note":"done"}),
            "12:00"
        )
        .is_err());
}

#[test]
fn patrol_log_is_utf8_and_display_is_limited_without_modifying_original_material() {
    let mut input = materials();
    let log = (0..55)
        .map(|n| format!("- [2026-10-15 12:{n:02}] 原记录{n}\n"))
        .collect::<String>();
    input.patrol_log = Some(log.clone());
    let plan =
        build_heartbeat_plan("Hana", "zh", "12:00", &input, &HeartbeatSnapshot::default()).unwrap();
    assert!(!plan.patrol.prompt.contains("原记录4\n"));
    assert!(plan.patrol.prompt.contains("原记录5\n"));
    assert!(plan.patrol.prompt.contains("原记录54"));
    assert_eq!(input.patrol_log.as_deref(), Some(log.as_str()));
    let write = plan
        .patrol
        .log_target
        .prepare_write(
            Some(&log),
            &json!({"status":"completed","note":"已完成中文巡检\n- [伪时间] 不能另起一条"}),
            "2026-10-15 12:30",
        )
        .unwrap();
    let output = String::from_utf8(write.content).unwrap();
    assert!(output.starts_with("- [2026-10-15 12:05] 原记录5\n"));
    assert_eq!(output.lines().filter(|l| l.starts_with("- [")).count(), 51);
    assert!(output.contains("已完成中文巡检 - [伪时间]"));
    assert_eq!(write.capability, "patrol_update_log.update");
}

#[test]
fn output_segments_preserve_current_locale_labels_and_cannot_traverse() {
    assert_eq!(output_paths(".. /", "zh").0, "OH-Works/Agent-activity");
    assert_eq!(output_paths("CON", "en").0, "OH-Works/agent-CON-activity");
    for (locale, suffix) in [
        ("zh-TW", "的巡檢"),
        ("ja", "の巡回"),
        ("ko", "의 순찰"),
        ("en", " Patrol"),
    ] {
        assert!(output_paths("Hana", locale)
            .1
            .ends_with(&format!("Hana{suffix}/patrol-log.md")));
    }
    let long = "花".repeat(60);
    let (activity, _) = output_paths(&long, "zh");
    let segment = activity
        .strip_prefix("OH-Works/")
        .unwrap()
        .strip_suffix("-activity")
        .unwrap();
    assert!(segment.len() <= 80);
    assert!(segment.chars().all(|c| c == '花'));
}

#[test]
fn daily_boundaries_are_local_and_failures_or_remaining_work_retry_after_one_hour() {
    let tz = FixedOffset::east_opt(8 * 60 * 60).unwrap();
    let before = tz.with_ymd_and_hms(2026, 10, 15, 3, 30, 0).unwrap();
    assert_eq!(initial_daily_delay_ms(&before).unwrap(), 35 * 60 * 1_000);
    let boundary = tz.with_ymd_and_hms(2026, 10, 15, 4, 5, 0).unwrap();
    assert_eq!(initial_daily_delay_ms(&boundary).unwrap(), 60_000);
    let after = tz.with_ymd_and_hms(2026, 10, 15, 10, 0, 0).unwrap();
    assert_eq!(initial_daily_delay_ms(&after).unwrap(), 60_000);
    assert_eq!(
        next_daily_delay_ms(&after, DailyFreshCompactResult::default()).unwrap(),
        (18 * 60 + 5) * 60 * 1_000
    );
    for result in [
        DailyFreshCompactResult {
            failed: true,
            ..Default::default()
        },
        DailyFreshCompactResult {
            retry: true,
            ..Default::default()
        },
        DailyFreshCompactResult {
            stale_remaining: 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            next_daily_delay_ms(&after, result).unwrap(),
            DAILY_RETRY_DELAY_MS
        );
    }
    // 同一UTC时刻在另一时区并非本地04:05；不能用服务器UTC日界替代用户日界。
    let utc = after.with_timezone(&FixedOffset::east_opt(0).unwrap());
    assert_eq!(
        initial_daily_delay_ms(&utc).unwrap(),
        (2 * 60 + 5) * 60 * 1_000
    );
}
