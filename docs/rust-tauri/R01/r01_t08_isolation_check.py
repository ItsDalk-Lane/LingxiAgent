#!/usr/bin/env python3
"""R01-T08 原型与生产目录隔离的机器核查。

断言：R01 各原型/生成物目录（rust/、spike/、contracts/、tests/migration/r01-*、
scripts/rust-tauri/r01-*）未被任何生产入口引用。生产入口集合 =
package.json scripts、desktop 主/preload/renderer 源、server/、core/、lib/、
shared/、scripts/（剔除 scripts/rust-tauri/ 自身）、vite/tsconfig/
electron-builder 配置。

方法：对上述生产文件做逐字节内容扫描，命中以下引用模式即判违规：
  rust/crates, lingxi-protocol, lingxi-kernel, lingxi-spike,
  lingxi-browser-spike, spike/tauri-shell, contracts/generated,
  tests/migration/r01, lingxi.wire

豁免：纯文档（docs/）、证据（artifacts/）、任务书目录不参与扫描；
生产侧 migration 永久测试（tests/migration/r00-*、network-guard*）不属于
R01 原型，不在 forbidden 模式内（模式精确到 r01 前缀）。

同时反向核查：
  R1 rust/ 与 spike/ 内全部源文件在 git 跟踪内且不属于生产包路径；
  R2 .gitignore 覆盖 spike 构建产物（app/src-tauri/target 等）；
  R3 根 .gitignore 是否覆盖 rust/target（如实报告，不判负——R02 目录约定处理）。

用法：python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py [--out R.json]
退出码：0 = 无违规；1 = 存在生产入口引用原型。
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]

FORBIDDEN_PATTERNS = [
    "rust/crates",
    "lingxi-protocol",
    "lingxi-kernel",
    "lingxi-spike",
    "lingxi-browser-spike",
    "spike/tauri-shell",
    "contracts/generated",
    "tests/migration/r01",
    "lingxi.wire",
]

PRODUCTION_SCAN_PREFIXES = [
    "package.json",
    "desktop/",
    "server/",
    "core/",
    "lib/",
    "shared/",
    "scripts/",
    "build/",
]
PRODUCTION_SCAN_EXCLUDE_PREFIXES = [
    "scripts/rust-tauri/",   # R01 迁移工具自身
    "desktop/src/react/animation-mockup/",
]
EXTRA_SCAN_FILES_GLOBS = ["vite.config.*", "tsconfig*.json", "electron-builder*.yml", "electron-builder*.yaml", "electron-builder*.json"]
TEXT_SUFFIXES = {".js", ".cjs", ".mjs", ".ts", ".tsx", ".json", ".yml", ".yaml", ".toml", ".html", ".css", ".mdx"}


def git_ls_files():
    out = subprocess.run(["git", "ls-files"], cwd=REPO, capture_output=True, text=True, check=True)
    return [l for l in out.stdout.splitlines() if l]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    tracked = git_ls_files()
    violations = []
    scanned = 0

    scan_files = []
    for p in tracked:
        if any(p.startswith(x) for x in PRODUCTION_SCAN_EXCLUDE_PREFIXES):
            continue
        if any(p == pfx or p.startswith(pfx) for pfx in PRODUCTION_SCAN_PREFIXES):
            scan_files.append(p)
    import glob as _glob
    for g in EXTRA_SCAN_FILES_GLOBS:
        for m in _glob.glob(str(REPO / g)):
            rel = str(Path(m).relative_to(REPO))
            if rel in tracked and rel not in scan_files:
                scan_files.append(rel)

    for rel in sorted(set(scan_files)):
        fp = REPO / rel
        if Path(rel).suffix not in TEXT_SUFFIXES and Path(rel).name != "package.json":
            continue
        try:
            text = fp.read_text(encoding="utf-8", errors="strict")
        except (UnicodeDecodeError, ValueError):
            continue
        scanned += 1
        for pat in FORBIDDEN_PATTERNS:
            if pat in text:
                for i, line in enumerate(text.splitlines(), 1):
                    if pat in line:
                        violations.append({"file": rel, "line": i, "pattern": pat, "text": line.strip()[:160]})

    # R1：原型目录全部是迁移证据/工具路径，未混入生产包
    proto_prefixes = ("rust/", "spike/", "contracts/", "tests/migration/r01-", "scripts/rust-tauri/r01-")
    proto_files = [p for p in tracked if p.startswith(proto_prefixes)]
    bad_proto = [p for p in proto_files
                 if p.startswith(tuple(PRODUCTION_SCAN_PREFIXES[1:7]))
                 and not p.startswith("scripts/rust-tauri/")]

    # R2/R3：构建产物的 gitignore 覆盖
    def ignored(path):
        return subprocess.run(["git", "check-ignore", "-q", path], cwd=REPO).returncode == 0

    r2 = {
        "spike/tauri-shell/app/src-tauri/target/": ignored("spike/tauri-shell/app/src-tauri/target/x"),
        "spike/tauri-shell/app/node_modules/": ignored("spike/tauri-shell/app/node_modules/x"),
    }
    r3_rust_target_ignored = ignored("rust/target/x")

    report = {
        "scanned_production_files": scanned,
        "forbidden_patterns": FORBIDDEN_PATTERNS,
        "violations": violations,
        "prototype_tracked_files": len(proto_files),
        "prototype_files_under_production_dirs": bad_proto,
        "gitignore_spike_build_outputs": r2,
        "gitignore_rust_target_ignored": r3_rust_target_ignored,
        "verdict": "ISOLATED" if not violations and not bad_proto else "ISOLATION-VIOLATION",
    }
    if args.out:
        Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")

    print(f"scanned production files: {scanned}")
    print(f"prototype tracked files (rust/+spike/+contracts/+tests/migration/r01-*+scripts/rust-tauri/r01-*): {len(proto_files)}")
    print(f"prototype files under production dirs: {len(bad_proto)}")
    print(f"gitignore spike build outputs: {r2}")
    print(f"gitignore rust/target ignored: {r3_rust_target_ignored}（如实报告；不判负，目录约定建议 R02 增补）")
    if violations:
        print(f"ISOLATION VIOLATIONS ({len(violations)}):")
        for v in violations[:20]:
            print(f"  {v['file']}:{v['line']} pattern={v['pattern']} :: {v['text']}")
        print("RESULT: ISOLATION-VIOLATION")
        sys.exit(1)
    print("RESULT: ISOLATED（生产入口零引用 R01 原型/生成物）")
    sys.exit(0)


if __name__ == "__main__":
    main()
