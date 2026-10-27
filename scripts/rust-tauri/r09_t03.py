#!/usr/bin/env python3
"""R09-T03 证据生产者：桌面基础能力等价（A05 用户动作 / A06 IPC 负向）。

套件（由 `r09_suites.py --suite <name>` 分派；阶段门内真实执行）：
  - `r09_t03_native_matrix`：原生能力模块真实单测 + 跨文件一致性（主题/i18n/
    托盘菜单/截图主题/窗口状态默认值/quick-chat 常量）逐项断言。
  - `r09_t03_ipc_negative`：A06 负向矩阵逐 case（`NEGATIVE_CASE ...` 行）+
    静态检查（源码零 shell spawn、窗口 label 门存在）+ 未注册/系统动作显式阻塞。

证据纪律（禁止假绿）：
  - 模块单测/负向矩阵是真实执行（rustc 编译 + test binary 退出码 + 行解析）；
  - 托盘/快捷键/通知/对话框/录屏等需要**已注册 Tauri 宿主 + 真实系统动作**的
    断言，在 host-core 合并注册前一律记 `blocked::*` 并让套件 exit 2（门真红），
    不冒充通过；
  - 隔离：`LINGXI_HOME`/`TMPDIR` 指 `/private/tmp/lingxi-r09-*`（Producer 内建
    真实 `~/.lingxi` 前后摘要断言）。

用法：
  python3 -B scripts/rust-tauri/r09_t03.py --suite r09_t03_native_matrix \
      --evidence /private/tmp/r09-t03-native-01
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import Producer, run, rustup, sha256_file, toolchain  # noqa: E402
from r09_gui_actions import (  # noqa: E402
    APP_ENV, GUI_ENV, MANUAL_ENV, WD_PORT_ENV, gui_cleared, load_plan, run_gui_plan,
)

NATIVE_DIR = ROOT / "desktop/src-tauri/src/native"
LOCALES_DIR = ROOT / "desktop/src/locales"
THEME_DATA = ROOT / "desktop/src/shared/theme-registry-data.json"
MAIN_CJS = ROOT / "desktop/main.cjs"
LEAF_CASE_PLAN = ROOT / "docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json"

HARNESS_CRATE = "native_host_harness"


# ── 工具 ────────────────────────────────────────────────────────────────


def build_native_harness(producer: Producer) -> Path:
    """把 `desktop/src-tauri/src/native/mod.rs` 作为独立 crate 根编译测试二进制。

    纯逻辑模块不依赖 tauri；`#[cfg(not(r09_standalone_test))]` 的 glue 被排除，
    因此本验证只覆盖可独立编译的纯逻辑（注册后由 host-core cargo build 全覆盖）。
    """
    target = producer.evidence / "harness"
    target.mkdir(parents=True, exist_ok=True)
    root_src = target / "lib.rs"
    mod_path = (NATIVE_DIR / "mod.rs").resolve()
    root_src.write_text(
        f'#[path = "{mod_path}"]\npub mod native;\n', encoding="utf-8")
    binary = target / HARNESS_CRATE
    producer.run_step(
        "build::native-harness",
        [rustup(), "run", toolchain(), "rustc", "--test", "--edition", "2021",
         "--cfg", "r09_standalone_test",
         "--crate-name", HARNESS_CRATE, str(root_src), "-o", str(binary)],
        timeout=600,
        env={"CARGO_TARGET_DIR": str(target / "target")},
    )
    return binary


def run_rust_tests(producer: Producer, binary: Path, filter_: str | None = None,
                   nocapture: bool = False) -> str:
    argv = [str(binary), "--test-threads", "1"]
    if nocapture:
        argv.append("--nocapture")
    if filter_:
        argv.append(filter_)
    step = "test::" + (filter_ or "all")
    process = producer.run_step(step, argv, timeout=600)
    return process.stdout + process.stderr


def rust_case(producer: Producer, binary: Path, case: str, test: str) -> bool:
    """逐 `leaf::...` case 精确跑对应单测；退出码 + `test result: ok` 双重判定。"""
    process = run([str(binary), "--exact", test, "--test-threads", "1"],
                  timeout=600, env=producer.env())
    output = process.stdout + process.stderr
    ok = process.returncode == 0 and f"test {test} ... ok" in output
    return producer.check(case, ok, detail={"test": test, "exitCode": process.returncode})


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def main_cjs_constants() -> dict:
    """从现役 main.cjs 提取 T03 对照常量（正则，不依赖行号）。"""
    text = read_text(MAIN_CJS)
    patterns = {
        "quick_chat_width": r"const QUICK_CHAT_WIDTH = (\d+);",
        "quick_chat_compact_height": r"const QUICK_CHAT_COMPACT_HEIGHT = (\d+);",
        "quick_chat_chat_height": r"const QUICK_CHAT_CHAT_HEIGHT = (\d+);",
        "quick_chat_min_width": r"const QUICK_CHAT_MIN_WIDTH = (\d+);",
        "quick_chat_min_height": r"const QUICK_CHAT_MIN_HEIGHT = (\d+);",
        "screenshot_scale": r"const SCREENSHOT_CAPTURE_SCALE = (\d+);",
        "screenshot_max_segment": r"const SCREENSHOT_MAX_SEGMENT = (\d+);",
        "screenshot_margin": r"const SCREENSHOT_SEGMENT_SCREEN_MARGIN = (\d+);",
    }
    found = {}
    for key, pattern in patterns.items():
        match = re.search(pattern, text)
        if match:
            found[key] = int(match.group(1))
    snippet = re.search(r"const SCREENSHOT_THEMES = \{(.*?)\n\};", text, re.S)
    themes = {}
    if snippet:
        for theme_match in re.finditer(
                r'"([a-z-]+)":\s*\{ width: (\d+), backgroundColor: "(#[0-9A-Fa-f]+)" \}',
                snippet.group(1)):
            themes[theme_match.group(1)] = {
                "width": int(theme_match.group(2)),
                "background": theme_match.group(3).upper(),
            }
    found["screenshot_themes"] = themes
    return found


def rust_theme_table() -> dict:
    text = read_text(NATIVE_DIR / "theme_sync.rs")
    table = {}
    for match in re.finditer(
            r'theme!\("([a-z-]+)", "(#[0-9A-Fa-f]+)", (true|false)\)', text):
        table[match.group(1)] = match.group(2).upper()
    return table


def rust_i18n_tray_strings() -> dict:
    """粗解析 i18n.rs 中 5 语言的 tray 文案（结构固定，用于跨文件对照）。"""
    text = read_text(NATIVE_DIR / "i18n.rs")
    block = text.split("pub fn tray_strings", 1)[1]
    entries = {}
    for locale_match in re.finditer(
            r'"([A-Za-z-]+)" => TrayStrings \{(.*?)\n        \},', block, re.S):
        locale = locale_match.group(1)
        body = locale_match.group(2)
        fields = {}
        for field_match in re.finditer(r'(\w+): "([^"]*)"', body):
            fields[field_match.group(1)] = field_match.group(2)
        entries[locale] = fields
    # 兜底分支 `_ => TrayStrings { ... }` 即 en（5 语言之一）
    fallback = re.search(r'_ => TrayStrings \{(.*?)\n        \},', block, re.S)
    if fallback and "en" not in entries:
        fields = {}
        for field_match in re.finditer(r'(\w+): "([^"]*)"', fallback.group(1)):
            fields[field_match.group(1)] = field_match.group(2)
        entries["en"] = fields
    return entries


def locale_tray_strings() -> dict:
    result = {}
    for locale in ["zh", "zh-TW", "ja", "ko", "en"]:
        data = read_json(LOCALES_DIR / f"{locale}.json")
        main = data.get("main", {})
        tray = main.get("tray", {})
        result[locale] = {
            "show": tray.get("show"),
            "settings": tray.get("settings"),
            "repair_artifacts": tray.get("repairArtifacts"),
            "quit": tray.get("quit"),
            "notification_ready": main.get("notification", {}).get("ready"),
        }
    return result


# ── 套件：native matrix ────────────────────────────────────────────────

LEAF_CASES: dict[str, str] = {
    # 每条 = 一个 R00 叶断言可绑定的专属 case → 支撑它的真实 Rust 单测
    "leaf::window-controls.caller-authorized":
        "native::window_controls::tests::only_main_and_settings_may_control_windows",
    "leaf::window-controls.state-broadcast":
        "native::window_controls::tests::toggle_and_event_names_match_incumbent",
    "leaf::window-state.sanitize-visible":
        "native::window_state::tests::visible_position_is_preserved_across_displays",
    "leaf::window-state.offscreen-recenter":
        "native::window_state::tests::offscreen_position_is_recentered_on_primary",
    "leaf::window-state.persisted-path":
        "native::window_state::tests::persistence_path_is_home_adjacent",
    "leaf::app-ready.idempotent":
        "native::app_ready::tests::ready_is_idempotent_and_hidden_at_login_stays_hidden",
    "leaf::app-ready.timeout-fallback":
        "native::app_ready::tests::init_timeout_forces_show_unless_hidden_at_login",
    "leaf::reload-restart.main-only":
        "native::reload_restart::tests::reload_is_main_only",
    "leaf::reload-restart.reentry-guard":
        "native::reload_restart::tests::restart_guard_blocks_reentry_and_updates",
    "leaf::window-lifecycle.close-to-tray":
        "native::window_lifecycle::tests::close_hides_unless_quitting_or_updating",
    "leaf::window-lifecycle.owned-only-cleanup":
        "native::window_lifecycle::tests::service_ownership_decides_termination",
    "leaf::window-lifecycle.window-all-closed":
        "native::window_lifecycle::tests::window_all_closed_respects_tray_availability",
    "leaf::window-lifecycle.activate-recreate":
        "native::window_lifecycle::tests::activate_recreates_only_after_service_ready",
    "leaf::single-instance.home-scoped":
        "native::single_instance::tests::lock_namespace_is_home_scoped",
    "leaf::single-instance.second-instance-plan":
        "native::single_instance::tests::second_instance_prefers_main_then_onboarding",
    "leaf::tray.menu-model":
        "native::tray::tests::menu_has_four_items_without_unsupported_repair",
    "leaf::tray.locale-rebuild":
        "native::tray::tests::menu_labels_follow_locale",
    "leaf::tray.icon-candidates":
        "native::tray::tests::icon_candidates_follow_platform_rules",
    "leaf::global-shortcut.registration-unavailable":
        "native::global_shortcut::tests::registration_result_shapes_are_stable",
    "leaf::global-shortcut.max-three-groups":
        "native::global_shortcut::tests::shortcut_selection_uses_override_then_legacy_then_default",
    "leaf::global-shortcut.test-register-cleanup":
        "native::global_shortcut::tests::test_register_plan_always_cleans_up",
    "leaf::notification.focus-suppression":
        "native::notification::tests::focus_policy_normalization",
    "leaf::notification.receipt-not-delivery":
        "native::notification::tests::receipts_distinguish_shown_from_unconfirmed",
    "leaf::notification.avatar-sanitized":
        "native::notification::tests::agent_id_and_avatar_paths_are_sanitized",
    "leaf::notification.click-focus-support-matrix":
        "native::notification::tests::click_focus_is_explicitly_unsupported",
    "leaf::keep-awake.state-machine":
        "native::keep_awake::tests::enable_starts_blocker_and_is_idempotent",
    "leaf::keep-awake.failure-not-fake":
        "native::keep_awake::tests::start_failure_never_reports_enabled",
    "leaf::auto-launch.opened-at-login":
        "native::auto_launch::tests::opened_at_login_rules_per_platform",
    "leaf::auto-launch.unsupported-platform":
        "native::auto_launch::tests::platform_support_matches_incumbent",
    "leaf::file-dialogs.cancel-shapes":
        "native::file_dialogs::tests::cancel_results_match_incumbent_shapes",
    "leaf::file-dialogs.skill-extension":
        "native::file_dialogs::tests::skill_selection_rules",
    "leaf::clipboard-edit.allowlist":
        "native::clipboard_edit::tests::allowed_set_is_exactly_four",
    "leaf::external-open.web-only":
        "native::external_open::tests::external_open_only_web_schemes",
    "leaf::external-open.receipt-shape":
        "native::external_open::tests::caller_policy_and_receipt_shape",
    "leaf::drag-out.shapes":
        "native::drag_out::tests::single_and_multiple_shapes",
    "leaf::file-drop.native-paths":
        "native::file_drop::tests::drop_paths_are_native_absolute_and_deduped",
    "leaf::trash.absolute-only":
        "native::trash::tests::path_resolution_matches_incumbent",
    "leaf::settings-window.main-modal-first":
        "native::settings_window::tests::main_modal_preferred_when_main_alive",
    "leaf::settings-window.crashed-main-fallback":
        "native::settings_window::tests::crashed_main_falls_back_to_settings_window",
    "leaf::settings-window.tab-validation":
        "native::settings_window::tests::tab_validation_is_syntactic",
    "leaf::settings-broadcast.side-effects":
        "native::settings_broadcast::tests::side_effects_are_not_optional",
    "leaf::settings-broadcast.whitelist":
        "native::settings_broadcast::tests::unknown_events_are_rejected",
    "leaf::quick-chat.grow-only":
        "native::quick_chat::tests::chat_mode_is_grow_only",
    "leaf::quick-chat.bounds-restore":
        "native::quick_chat::tests::resolve_bounds_restores_and_sanitizes",
    "leaf::quick-chat-handoff.ack-target-match":
        "native::quick_chat_handoff::tests::ack_must_match_request_and_session",
    "leaf::quick-chat-handoff.focus-before-hide":
        "native::quick_chat_handoff::tests::focus_must_be_confirmed_before_hiding",
    "leaf::theme-sync.quick-chat-transparent":
        "native::theme_sync::tests::sync_plan_themes_caller_window_only_and_quick_chat_transparent",
    "leaf::i18n.locale-resolution":
        "native::i18n::tests::locale_resolution_matches_incumbent",
    "leaf::i18n.tray-locale-rebuild":
        "native::i18n::tests::locale_change_rebuilds_tray_menu",
    "leaf::app-info.platform-literal":
        "native::app_info::tests::tauri_naming_is_mapped_not_passed_through",
    "leaf::splash.sender-and-page-gate":
        "native::splash::tests::page_url_whitelist",
    "leaf::splash.avatar-path-scope":
        "native::splash::tests::avatar_segments_and_paths_are_home_scoped",
    "leaf::onboarding.rust-service-unsupported":
        "native::onboarding::tests::rust_service_is_explicitly_unsupported",
    "leaf::onboarding.preview-no-write":
        "native::onboarding::tests::debug_commands_are_debug_only_and_preview_has_no_side_effects",
}


def host_leaf_classification() -> dict:
    """从 R00 冻结账 + 清点 §9 重新计算 30 个 host 叶的权威分类（plan 的 ground truth）。"""
    ledger = read_json(ROOT / "docs/rust-tauri/R09/R09_LEAF_LEDGER.json")
    fsa = {s["feature_id"]: s for s in read_json(
        ROOT / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")["supplemental_scenarios"]}
    inventory = read_text(ROOT / "docs/rust-tauri/R09/T03_HOST_CAPABILITY_INVENTORY.md")
    section = inventory.split("## 9. 46 叶关联表")[1].split("## 10.")[0]
    mapping = {}
    for line in section.splitlines():
        match = re.match(r"\|\s*`([^`]+)`\s*\|\s*([^|]+)\|\s*(.+?)\s*\|", line)
        if match and match.group(1).startswith("F-"):
            mapping[match.group(1)] = match.group(3).strip()
    result = {}
    for leaf in ledger["leaves"]:
        if "R09-T03" not in leaf["executionTaskIds"]:
            continue
        caps = re.findall(r"`(host:[a-z0-9-]+)`", mapping.get(leaf["featureId"], ""))
        if not caps:
            continue
        category = re.match(r"F-D23-([A-Z_]+)-", leaf["featureId"]).group(1)
        host_class = "HOST_NATIVE" if category == "DESKTOP_BEHAVIOR" else "HOST_BRIDGE"
        result[leaf["id"]] = {
            "hostClass": host_class,
            "featureId": leaf["featureId"],
            "assertions": fsa[leaf["featureId"]]["assertions"],
            "capabilities": caps,
            "category": category,
        }
    return result


def validate_leaf_case_plan(producer: Producer) -> int:
    """校验 T03_LEAF_CASE_PLAN.json 是权威、完整、可执行的逐断言映射计划。

    不启动 GUI、不依赖宿主注册：叶集合/断言数/命名/覆盖/逻辑 case 存在性/GUI 门/
    人工步骤/阻塞原因全部机器校验（计划本身的质量门）。
    """
    if not LEAF_CASE_PLAN.is_file():
        producer.check("plan::file-exists", False, detail={"path": str(LEAF_CASE_PLAN)})
        return 1
    plan = load_plan(LEAF_CASE_PLAN)
    ground = host_leaf_classification()
    producer.check("plan::schema", plan.get("schema") == "lingxi.r09.t03-leaf-case-plan.v1",
                   detail={"schema": plan.get("schema")})
    producer.check("plan::scope-counts",
                   plan.get("scope", {}).get("sharedLeaves") == 46
                   and plan.get("scope", {}).get("hostLeaves") == len(ground) == 30
                   and plan.get("scope", {}).get("hostNative") == 22
                   and plan.get("scope", {}).get("hostBridge") == 8
                   and plan.get("scope", {}).get("serverReactExcluded") == 16,
                   detail=plan.get("scope"))
    planned_leaf_ids = [leaf["leafId"] for leaf in plan.get("leaves", [])]
    producer.check("plan::leaf-set-equals-authoritative-30",
                   sorted(planned_leaf_ids) == sorted(ground),
                   detail={"missing": sorted(set(ground) - set(planned_leaf_ids)),
                           "extra": sorted(set(planned_leaf_ids) - set(ground))})
    class_mismatch = [leaf["leafId"] for leaf in plan.get("leaves", [])
                      if leaf.get("hostClass") != ground.get(leaf["leafId"], {}).get("hostClass")
                      or leaf.get("featureId") != ground.get(leaf["leafId"], {}).get("featureId")]
    producer.check("plan::host-class-and-feature-match-authoritative", not class_mismatch,
                   detail={"mismatch": class_mismatch})

    total_assertions = sum(len(leaf.get("assertions", [])) for leaf in plan.get("leaves", []))
    assertion_text_mismatch = []
    naming_violations = []
    missing_plan_fields = []
    manual_without_steps = []
    blocked_without_reason = []
    logic_cases_missing = []
    case_names = set()
    known_logic = set(LEAF_CASES)
    ipc_matrix = read_text(NATIVE_DIR / "ipc_negative_matrix.rs")
    known_logic |= set(re.findall(r'case!\("([^"]+)"', ipc_matrix))
    for leaf in plan.get("leaves", []):
        expected_assertions = ground.get(leaf["leafId"], {}).get("assertions", [])
        if len(leaf.get("assertions", [])) != len(expected_assertions):
            missing_plan_fields.append({"leafId": leaf["leafId"], "reason": "assertion-count"})
        for index, assertion in enumerate(leaf.get("assertions", []), 1):
            if assertion.get("assertionIndex") != index:
                missing_plan_fields.append({"leafId": leaf["leafId"], "reason": "assertion-index"})
            if index <= len(expected_assertions) and assertion.get("assertionText") != expected_assertions[index - 1]:
                assertion_text_mismatch.append(f"{leaf['leafId']}#{index}")
            for field in ("plannedActionCase", "requiredObservation", "needsVisibleGUI",
                          "blockedReason", "implementationStatus", "existingLogicCases"):
                if field not in assertion:
                    missing_plan_fields.append({"leafId": leaf["leafId"], "field": field, "index": index})
            pattern = re.compile(r"^leaf:R00-T02-LA-[0-9A-F]+::a\d+-[a-z0-9-]+$")
            if not pattern.match(str(assertion.get("plannedActionCase", ""))):
                naming_violations.append(str(assertion.get("plannedActionCase")))
            case_names.add(assertion.get("plannedActionCase"))
            if assertion.get("implementationStatus") == "manual" and not assertion.get("manualSteps"):
                manual_without_steps.append(assertion.get("plannedActionCase"))
            if str(assertion.get("implementationStatus", "")).startswith("blocked") \
                    and not assertion.get("blockedReason"):
                blocked_without_reason.append(assertion.get("plannedActionCase"))
            for logic_case in assertion.get("existingLogicCases", []) or []:
                if logic_case not in known_logic:
                    logic_cases_missing.append(logic_case)
    producer.check("plan::assertions-60", total_assertions == 60,
                   detail={"total": total_assertions})
    producer.check("plan::assertion-text-verbatim", not assertion_text_mismatch,
                   detail={"mismatch": assertion_text_mismatch})
    producer.check("plan::required-fields", not missing_plan_fields,
                   detail={"issues": missing_plan_fields})
    producer.check("plan::planned-case-naming", not naming_violations,
                   detail={"violations": naming_violations})
    producer.check("plan::planned-case-unique", len(case_names) == total_assertions,
                   detail={"unique": len(case_names), "total": total_assertions})
    producer.check("plan::every-assertion-has-action-case",
                   all(a.get("plannedActionCase") for leaf in plan.get("leaves", [])
                       for a in leaf.get("assertions", [])))
    producer.check("plan::manual-has-steps", not manual_without_steps,
                   detail={"missing": manual_without_steps})
    producer.check("plan::blocked-has-reason", not blocked_without_reason,
                   detail={"missing": blocked_without_reason})
    producer.check("plan::logic-case-references-exist", not logic_cases_missing,
                   detail={"missing": sorted(set(logic_cases_missing))})
    statuses = {}
    for leaf in plan.get("leaves", []):
        for assertion in leaf.get("assertions", []):
            status = assertion.get("implementationStatus")
            statuses[status] = statuses.get(status, 0) + 1
    producer.check("plan::stats-present",
                   plan.get("stats", {}).get("assertionsWithPlannedAction") == total_assertions,
                   detail={"statuses": statuses, "stats": plan.get("stats")})
    gui_gate = plan.get("guiGate", {})
    producer.check("plan::gui-gate-envs",
                   gui_gate.get("env") == GUI_ENV
                   and gui_gate.get("appBinaryEnv") == APP_ENV
                   and gui_gate.get("webdriverPortEnv") == WD_PORT_ENV
                   and gui_gate.get("manualEvidenceDirEnv") == MANUAL_ENV,
                   detail=gui_gate)
    return 0


def run_native_matrix(producer: Producer) -> int:
    validate_leaf_case_plan(producer)
    binary = build_native_harness(producer)

    # 1) 全量单测真实退出码
    output = run_rust_tests(producer, binary)
    match = re.search(r"test result: ok\. (\d+) passed; 0 failed", output)
    total = int(match.group(1)) if match else 0
    producer.check("native::all-unit-tests", match is not None and total > 0, detail={
        "passed": total,
        "testResultLine": match.group(0) if match else None,
    })

    # 2) 逐 leaf case → 专属单测（供 Lead 登记 FULL_BINDINGS）
    for case, test in LEAF_CASES.items():
        rust_case(producer, binary, case, test)

    # 3) 跨文件一致性：主题表 vs theme-registry-data.json
    registry = read_json(THEME_DATA)
    registry_themes = {
        theme_id: entry["backgroundColor"].upper()
        for theme_id, entry in registry["themes"].items()
    }
    module_themes = rust_theme_table()
    producer.check(
        "parity::theme-table-vs-registry",
        module_themes == registry_themes,
        detail={"registry": registry_themes, "module": module_themes},
    )
    producer.check(
        "parity::theme-defaults",
        registry["defaultTheme"] == "warm-paper"
        and registry["autoLightDefault"] == "warm-paper"
        and registry["autoDarkDefault"] == "midnight"
        and registry["legacyThemeAliases"].get("claude-design") == "new-warm-paper",
        detail={"defaultTheme": registry["defaultTheme"]},
    )

    # 4) 托盘 i18n 文案 vs locales/*.json
    module_i18n = rust_i18n_tray_strings()
    locale_i18n = locale_tray_strings()
    producer.check(
        "parity::tray-i18n-five-locales",
        set(module_i18n) == set(locale_i18n) and module_i18n == locale_i18n,
        detail={"module": module_i18n, "locales": locale_i18n},
    )

    # 5) 现役 main.cjs 常量 vs Rust 模块常量
    constants = main_cjs_constants()
    quick_chat_rs = read_text(NATIVE_DIR / "quick_chat.rs")
    expected_quick = {
        "QUICK_CHAT_WIDTH": constants.get("quick_chat_width"),
        "QUICK_CHAT_COMPACT_HEIGHT": constants.get("quick_chat_compact_height"),
        "QUICK_CHAT_CHAT_HEIGHT": constants.get("quick_chat_chat_height"),
        "QUICK_CHAT_MIN_WIDTH": constants.get("quick_chat_min_width"),
        "QUICK_CHAT_MIN_HEIGHT": constants.get("quick_chat_min_height"),
    }
    quick_ok = all(
        name in quick_chat_rs and f"pub const {name}: u32 = {value};" in quick_chat_rs
        for name, value in expected_quick.items()
    )
    producer.check("parity::quick-chat-constants", quick_ok, detail=expected_quick)

    screenshot_rs = read_text(NATIVE_DIR / "screenshot.rs")
    screenshot_ok = (
        f"pub const CAPTURE_SCALE: u32 = {constants.get('screenshot_scale')};" in screenshot_rs
        and f"pub const MAX_SEGMENT_HEIGHT: u32 = {constants.get('screenshot_max_segment')};" in screenshot_rs
        and f"pub const SEGMENT_SCREEN_MARGIN: u32 = {constants.get('screenshot_margin')};" in screenshot_rs
    )
    producer.check("parity::screenshot-constants", screenshot_ok, detail=constants)

    # 6) 截图 6 主题 vs 现役 SCREENSHOT_THEMES
    module_shot = {}
    for match in re.finditer(
            r'ScreenshotTheme \{\s*id: "([a-z-]+)",\s*width: (\d+),\s*background_hex: "(#[0-9A-Fa-f]+)",\s*\}',
            screenshot_rs):
        module_shot[match.group(1)] = {
            "width": int(match.group(2)),
            "background": match.group(3).upper(),
        }
    producer.check(
        "parity::screenshot-themes-vs-electron",
        module_shot == constants.get("screenshot_themes"),
        detail={"module": module_shot, "electron": constants.get("screenshot_themes")},
    )

    # 7) 窗口状态默认值 vs window-state.cjs/main.cjs
    window_state_rs = read_text(NATIVE_DIR / "window_state.rs")
    producer.check(
        "parity::window-state-defaults",
        "default_width: 960.0" in window_state_rs
        and "default_height: 820.0" in window_state_rs
        and "min_width: 420.0" in window_state_rs
        and "min_height: 500.0" in window_state_rs
        and "min_visible_width: 80.0" in window_state_rs
        and "min_visible_height: 60.0" in window_state_rs
        and 'WINDOW_STATE_RELATIVE_PATH: &str = "user/window-state.json"' in window_state_rs,
    )

    # 8) 源码零 shell spawn（T03 不得把畸形参数转 shell；系统动作走 crate API）
    spawn_hits = []
    for path in sorted(NATIVE_DIR.glob("*.rs")):
        text = read_text(path)
        for pattern in ["std::process::Command", "tokio::process", "Command::new", "sh -c", "powershell"]:
            if pattern in text:
                spawn_hits.append({"file": path.name, "pattern": pattern})
    producer.check(
        "static::no-shell-spawn-in-native-modules",
        not spawn_hits,
        detail={"hits": spawn_hits},
    )

    # 9) 每个能力模块都带单测（能力矩阵骨架的机器约束）
    modules = [path for path in NATIVE_DIR.glob("*.rs")
               if path.name not in {"mod.rs", "errors.rs", "ipc_negative_matrix.rs"}]
    missing_tests = [path.name for path in modules if "#[cfg(test)]" not in read_text(path)]
    producer.check(
        "static::every-capability-module-has-tests",
        not missing_tests,
        detail={"modules": len(modules), "missingTests": missing_tests},
    )

    # 10) 真实系统动作：GUI-cleared 批次真实执行；否则逐叶 blocked（诚实红）
    plan = load_plan(LEAF_CASE_PLAN)
    gui_stats = run_gui_plan(producer, plan) if gui_cleared() else None
    if gui_stats is None:
        for leaf in plan.get("leaves", []):
            producer.check(f"blocked::gui-cleared::{leaf['leafId']}", False, detail={
                "blocked": True,
                "reason": f"未提供 {GUI_ENV}=1 / {APP_ENV}（visible 批次执行真实动作）",
                "plannedActionCases": [a["plannedActionCase"] for a in leaf["assertions"]],
            })
        producer.assert_real_home_untouched()
        return producer.finish(status="FAIL", extra={
            "blockedKind": "BLOCKED_PENDING_VISIBLE_GUI",
            "blockedLeafCount": len(plan.get("leaves", [])),
            "leafCasePlan": str(LEAF_CASE_PLAN.relative_to(ROOT)),
            "note": "纯逻辑/IPC 负向/计划完整性已真实通过；真实动作需 LINGXI_R09_GUI_CLEARED=1 批次",
        })
    producer.assert_real_home_untouched()
    if gui_stats["failed"] or gui_stats["blocked"] or gui_stats["manual"] > gui_stats["manualPassed"]:
        return producer.finish(status="FAIL", extra={
            "guiStats": gui_stats,
            "blockedKind": "VISIBLE_BATCH_PARTIAL",
        })
    return producer.finish(status="PASS", extra={"guiStats": gui_stats})


# ── 套件：IPC 负向矩阵（A06）────────────────────────────────────────────

def run_ipc_negative(producer: Producer) -> int:
    binary = build_native_harness(producer)
    output = run_rust_tests(
        producer, binary,
        filter_="native::ipc_negative_matrix::tests::negative_matrix_emits_evidence",
        nocapture=True)

    lines = [line for line in output.splitlines() if line.startswith("NEGATIVE_CASE ")]
    seen = set()
    for line in lines:
        parts = line.split()
        # NEGATIVE_CASE <name> <command> <expect> <actual> OK|FAIL
        if len(parts) != 6:
            producer.check(f"negative::malformed::{line}", False, detail={"line": line})
            continue
        _, name, command, expect, actual, verdict = parts
        ok = verdict == "OK" and actual == expect
        seen.add(name)
        producer.check(name, ok, detail={"command": command, "expect": expect, "actual": actual})
    producer.check("negative::matrix-emitted", len(seen) >= 40, detail={"cases": len(seen)})

    # 静态边界：命令面零 shell spawn + 窗口 label 门在所有高风险模块中出现
    label_gate_modules = [
        "window_controls.rs", "app_ready.rs", "reload_restart.rs", "notification.rs",
        "clipboard_edit.rs", "drag_out.rs", "external_open.rs", "trash.rs",
        "settings_window.rs", "settings_broadcast.rs", "quick_chat.rs",
        "splash.rs", "onboarding.rs", "speech.rs", "screenshot.rs",
    ]
    missing_gates = [name for name in label_gate_modules
                     if "require_role" not in read_text(NATIVE_DIR / name)
                     and "require_exact_label" not in read_text(NATIVE_DIR / name)
                     and "validate_label_syntax" not in read_text(NATIVE_DIR / name)]
    producer.check("negative::window-label-gates-present", not missing_gates,
                   detail={"missing": missing_gates})

    producer.check(
        "negative::no-process-spawn",
        all("Command::new" not in read_text(path)
            for path in NATIVE_DIR.glob("*.rs")),
    )

    blocked = [
        ("ipc::registered-host-negative-invoke", "命令面已注册；逐窗口 invoke 负向矩阵（ACL 拒绝 + 无副作用）需真实 GUI/wdio（Lead GUI 禁令；T06 运行期矩阵）"),
        ("ipc::acl-denied-window", "未授权窗口调用必须被 Tauri ACL 拒绝；capability 配置已就绪（T06），真实宿主拒绝矩阵需 GUI/wdio"),
        ("ipc::no-child-process-observed", "真实 IPC 注入后进程表探针（无子进程/无 shell）需真实宿主会话（Lead GUI 禁令）"),
    ]
    for case, reason in blocked:
        producer.check(f"blocked::{case}", False, detail={"blocked": True, "reason": reason})
    producer.assert_real_home_untouched()
    return producer.finish(status="FAIL", extra={
        "blockedPendingRegistration": [reason for _, reason in blocked],
        "blockedKind": "BLOCKED_GUI_SMOKE",
    })


# ── 分派 ────────────────────────────────────────────────────────────────

def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    try:
        producer.check("isolation::real-home-provided",
                       bool(producer.env().get("LINGXI_HOME")))
        if suite == "r09_t03_native_matrix":
            code = run_native_matrix(producer)
        elif suite == "r09_t03_ipc_negative":
            code = run_ipc_negative(producer)
        else:
            code = producer.fail(f"未知 T03 套件: {suite}")
        # 隔离目录摘要写入证据
        (producer.evidence / "runtime.json").write_text(json.dumps({
            "schema": "lingxi.r09.runtime.v1",
            "home": str(producer.home),
            "tmp": str(producer.tmp),
            "nativeModules": sorted(path.name for path in NATIVE_DIR.glob("*.rs")),
            "mainCjsSha256": sha256_file(MAIN_CJS),
        }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        return code
    except Exception as error:  # noqa: BLE001 - 生产者必须真实失败并写回执
        return producer.fail(f"{type(error).__name__}: {error}")


if __name__ == "__main__":
    args = sys.argv[1:]
    if "--suite" not in args or "--evidence" not in args:
        print("usage: r09_t03.py --suite <name> --evidence <dir>", file=sys.stderr)
        sys.exit(64)
    suite_name = args[args.index("--suite") + 1]
    evidence_dir = Path(args[args.index("--evidence") + 1])
    if not evidence_dir.is_absolute():
        evidence_dir = ROOT / evidence_dir
    sys.exit(run_suite(suite_name, evidence_dir))
