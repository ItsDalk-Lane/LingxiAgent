#!/usr/bin/env python3
"""R07 UI/CLI 案例生产者：运行选定的 vitest 前端/CLI 测试文件，把结果写成
SUITES/r07_ui_vitest/{output.log,result.json}（与 cargo 套件同布局，供
r07_leaf_cases.py 统一解析）。

- 用 vitest JSON reporter 取每个断言的状态；output.log 行格式与 libtest 一致：
  `test <file-stem>::<fullName> ... ok|FAILED`。
- 任一测试失败/超时 → 退出码非 0；result.json.exitCode 如实记录（套件失败时
  生产者仍写日志，叶合同按「套件退出非 0 即案例不成立」语义判红）。
- 排除文件为既有基线红或需显式 live 环境者（R06SkillsRustLive 需 R06_SKILLS_*
  真实服务；R02RustWebSocket 基线红）——它们不属于本套件的可绑定证据。
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

FILES = [
    # 设置容器与各页
    "desktop/src/react/__tests__/settings/SettingsContent.test.tsx",
    "desktop/src/react/settings/__tests__/SettingsContent.test.tsx",
    "desktop/src/react/__tests__/settings/AccessTab.test.tsx",
    "desktop/src/react/settings/__tests__/R02AccessRoute.test.tsx",
    "desktop/src/react/settings/__tests__/R02AccessCredentialLeaf.test.tsx",
    "desktop/src/react/__tests__/settings/UsageTab.test.tsx",
    "desktop/src/react/__tests__/settings/observability/",
    "desktop/src/react/__tests__/settings/ProvidersTab.test.tsx",
    "desktop/src/react/__tests__/settings/ApiKeyCredentials.test.tsx",
    "desktop/src/react/__tests__/settings/OAuthCredentials.test.tsx",
    "desktop/src/react/__tests__/settings/ModelWidget.test.tsx",
    "desktop/src/react/__tests__/settings/BridgeTab.credentials.test.tsx",
    "desktop/src/react/__tests__/settings/BridgeTab.permission-mode.test.tsx",
    "desktop/src/react/__tests__/settings/R07BridgeRustUI.test.tsx",
    "desktop/src/react/__tests__/settings/R07BrowserRustUI.test.tsx",
    "desktop/src/react/__tests__/settings/useBridgeState.snapshot.test.tsx",
    "desktop/src/react/settings/__tests__/R02SharingLeaf.test.tsx",
    "desktop/src/react/__tests__/settings/WorkTab.test.tsx",
    "desktop/src/react/settings/__tests__/skill-scroll-contract.test.ts",
    # 频道页与连接
    "desktop/src/react/__tests__/stores/channel-actions.test.ts",
    "tests/channels-route.test.ts",
    "desktop/src/react/__tests__/stores/connection-slice.test.ts",
    "desktop/src/react/__tests__/app-init.test.ts",
    # 桌面行为（资源事件/查看器资源）
    "desktop/src/__tests__/viewer-window-resource-events.test.ts",
    "desktop/src/tauri-resources/__tests__/resource-desktop.test.tsx",
    # CLI
    "tests/cli-args.test.ts",
    "tests/cli-chat.test.ts",
    "tests/cli-chat-runtime.test.ts",
    "tests/cli-data.test.ts",
    "tests/cli-local-server.test.ts",
    "tests/cli-server-runner.test.ts",
    "tests/cli-rust-service.test.ts",
    "tests/cli-bundle.test.ts",
    "tests/cli-agent-selection.test.ts",
    "tests/cli-abort-contract.test.ts",
    # 即梦插件适配（TS 侧）
    "tests/jimeng-cli-adapter.test.ts",
    "tests/jimeng-cli-provider.test.ts",
    "tests/jimeng-cli-capabilities.test.ts",
    # R07 收口：对话工具面（run_tools/ptc 沙箱/run_code/security_scan/
    # ast_grep/ast_edit/lsp 的工具契约、权限与沙箱边界真实测试）
    "lib/tools/__tests__/run-code-tool.test.ts",
    "lib/tools/__tests__/ptc-tool.test.ts",
    "lib/tools/__tests__/security-scan-tool.test.ts",
    "lib/sandbox/__tests__/ast-tools.test.ts",
    "tests/lsp-tool.test.ts",
    "tests/learn-lesson-tool.test.ts",
]


def main(argv: list[str]) -> int:
    if "--evidence" not in argv:
        raise SystemExit("--evidence <root> required")
    evidence = Path(argv[argv.index("--evidence") + 1])
    out_dir = evidence / "SUITES" / "r07_ui_vitest"
    out_dir.mkdir(parents=True, exist_ok=True)
    report = Path("/private/tmp/r07-ui-vitest-report.json")
    started = time.time()
    proc = subprocess.run(
        ["npx", "vitest", "run", *FILES,
         "--reporter=json", f"--outputFile={report}"],
        cwd=ROOT, capture_output=True, text=True,
    )
    lines: list[str] = []
    passed = failed = 0
    try:
        data = json.loads(report.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        data = {}
    for test_file in data.get("testResults", []):
        stem = Path(test_file.get("name", "?")).stem
        for assertion in test_file.get("assertionResults", []):
            name = f"{stem}::{assertion.get('fullName', '')}".strip()
            if assertion.get("status") == "passed":
                lines.append(f"test {name} ... ok")
                passed += 1
            else:
                lines.append(f"test {name} ... FAILED")
                failed += 1
    if not lines:
        lines.append("test suite_load ... FAILED")
        failed += 1
    (out_dir / "output.log").write_text("\n".join(lines) + "\n", encoding="utf-8")
    exit_code = 0 if (proc.returncode == 0 and failed == 0 and passed > 0) else 1
    (out_dir / "result.json").write_text(json.dumps({
        "schema": "lingxi.r07.ui-suite-receipt.v1",
        "suite": "r07_ui_vitest",
        "exitCode": exit_code,
        "vitestExitCode": proc.returncode,
        "testsPassed": passed,
        "testsFailed": failed,
        "durationSecs": round(time.time() - started, 3),
    }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"ui suite: {passed} passed, {failed} failed (exit {exit_code})")
    return exit_code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
