#!/usr/bin/env python3
"""R00-T08｜R00-A15 预存失败可重现校验器。

用法：python3 -B docs/rust-tauri/R00/r00_t08_a15_verify_repro.py <log-r1> <log-r2>

对同条件两次复现运行的 vitest 原始日志提取失败用例集合，验证：
  1) 两次运行失败集合完全一致（可重现）；
  2) 失败集合与登记的预存失败（审计封印家族，见下）完全一致；
  3) 每个失败不因本次分类被改写为通过或删除（日志原样保留，本脚本只读）。

输入路径必须在仓库根目录内（规范化后校验，拒绝越界）。

预存失败登记（R00-T08 分类：预存[相对 T08]；根因：VERIFIED_SOURCE_SHA=46f12ab1c
的审计白名单不含 R00 迁移产生的 docs/artifacts/tests/scripts 增量；这些用例自
R00 首个提交起即失败，非 T08 改动引入；处置遵循 AGENTS.md：不为变绿扩白名单，
封印推进留待获准提交流程）：
  - tests/post-verification-audit-seal.test.ts > post-verification audit seal (diff guard) > changes since VERIFIED_SOURCE_SHA are audit-only (allowlist enforced)
  - tests/round2-delivery-evidence.test.ts > R10 round2 交付证据契约 > R10-03: 当前源码摘要可复算，且存在同摘要、执行期间未漂移的绿色门禁
  - tests/round2-delivery-evidence.test.ts > R10 round2 交付证据契约 > R10-04: tracked 与 untracked 源文件全部进入 manifest，证据/报告/补丁排除规则明示
  - tests/round3-delivery-evidence.test.ts > round3 C01-C03 候选交付证据契约 > round3: 源码 manifest 可复算，且覆盖本轮新增源码与测试

退出码：0=可重现且与登记一致；1=不一致（输出差异明细）；2=用法/输入错误。
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]

EXPECTED_FAILURES = {
    "tests/post-verification-audit-seal.test.ts > post-verification audit seal (diff guard) > changes since VERIFIED_SOURCE_SHA are audit-only (allowlist enforced)",
    "tests/round2-delivery-evidence.test.ts > R10 round2 交付证据契约 > R10-03: 当前源码摘要可复算，且存在同摘要、执行期间未漂移的绿色门禁",
    "tests/round2-delivery-evidence.test.ts > R10 round2 交付证据契约 > R10-04: tracked 与 untracked 源文件全部进入 manifest，证据/报告/补丁排除规则明示",
    "tests/round3-delivery-evidence.test.ts > round3 C01-C03 候选交付证据契约 > round3: 源码 manifest 可复算，且覆盖本轮新增源码与测试",
}

FAIL_LINE = re.compile(r"^ FAIL  (\S+\.test\.ts) > (.+)$", re.MULTILINE)


def safe_log_path(raw: str) -> Path:
    p = Path(raw).resolve()
    if not p.is_relative_to(REPO_ROOT) or not p.is_file():
        raise SystemExit(f"input path outside repo or missing: {raw}")
    return p


def failures(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    return {f"{m.group(1)} > {m.group(2).strip()}" for m in FAIL_LINE.finditer(text)}


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: r00_t08_a15_verify_repro.py <log-r1> <log-r2>", file=sys.stderr)
        return 2
    r1 = failures(safe_log_path(argv[1]))
    r2 = failures(safe_log_path(argv[2]))
    ok = True
    if r1 != r2:
        print(f"REPRO-MISMATCH only-r1={sorted(r1 - r2)} only-r2={sorted(r2 - r1)}")
        ok = False
    if r1 != EXPECTED_FAILURES:
        print(f"REGISTRY-MISMATCH unexpected={sorted(r1 - EXPECTED_FAILURES)} "
              f"missing={sorted(EXPECTED_FAILURES - r1)}")
        ok = False
    if ok:
        print(f"A15-REPRO-VERIFIED failures={len(r1)} identical_runs=2 "
              f"classification=preexisting(relative-to-T08) "
              f"root_cause=audit-seal-allowlist-excludes-R00-migration-files "
              f"VERIFIED_SOURCE_SHA=46f12ab1c5bc00f02a685346a3efc1b473590393")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
