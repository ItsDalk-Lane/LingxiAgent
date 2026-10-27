#!/usr/bin/env python3
"""R09 证据生产者的共享工具（不是验收语义本身）。

提供的都是可复算、可审计的小工具：
  - 仓库根/工具链解析：所有 Rust 命令经 rustup 锁定 1.98.1 执行；
  - 隔离运行时：LINGXI_HOME/TMPDIR 指向 /private/tmp 下的一次性目录，
    并采集真实 ~/.lingxi 前后摘要，供“不得触碰真实数据”断言；
  - 命令执行与回执：真实 argv/退出码/耗时/日志摘要写入证据目录；
  - 逐断言案例记录：`lingxi.leaf-case-results.v1` 案例文件（xtask 门读取）；
  - 文件/目录摘要：源码与产物的可复算 SHA-256。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import tempfile
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
RUNTIME_ROOT = Path(os.environ.get("LINGXI_R09_RUNTIME", "/private/tmp/lingxi-r09-runtime"))


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_tree(root: Path, skip_dirs: tuple[str, ...] = ()) -> dict:
    """可复算源码清单：相对路径 → sha256（目录/符号链接显式拒绝）。"""
    root = root.resolve()
    entries: dict[str, str] = {}
    for current, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in skip_dirs)
        for name in sorted(files):
            path = Path(current) / name
            if path.is_symlink():
                raise RuntimeError(f"refusing symlink in source manifest: {path}")
            entries[str(path.relative_to(root))] = sha256_file(path)
    return entries


def tree_digest(entries: dict) -> str:
    digest = hashlib.sha256()
    for name in sorted(entries):
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update(entries[name].encode())
        digest.update(b"\0")
    return digest.hexdigest()


def toolchain() -> str:
    content = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    for line in content.splitlines():
        line = line.strip()
        if line.startswith("channel"):
            return line.split("=", 1)[1].strip().strip('"')
    raise RuntimeError("rust-toolchain.toml has no pinned channel")


def rustup() -> str:
    candidates = [os.environ.get("RUSTUP_BIN"),
                  str(Path.home() / ".cargo/bin/rustup"),
                  "/opt/homebrew/bin/rustup"]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return candidate
    raise RuntimeError("locked Rust toolchain requires rustup (absolute path)")


def cargo(argv: list[str], timeout: int = 3600, env: dict | None = None) -> subprocess.CompletedProcess:
    """锁定工具链的 cargo；argv 为 cargo 子命令参数。"""
    command = [rustup(), "run", toolchain(), "cargo", *argv]
    return run(command, timeout=timeout, env=env)


def rustup_run(argv: list[str], timeout: int = 3600,
               env: dict | None = None) -> subprocess.CompletedProcess:
    """锁定工具链下执行任意 rust 工具（rustc/cargo/…）。"""
    return run([rustup(), "run", toolchain(), *argv], timeout=timeout, env=env)


def run(argv: list[str], timeout: int = 3600,
        env: dict | None = None) -> subprocess.CompletedProcess:
    merged = dict(os.environ)
    merged.update({
        "CARGO_TERM_COLOR": "never",
        "RUST_BACKTRACE": "1",
    })
    if env:
        merged.update(env)
    return subprocess.run(argv, cwd=ROOT, capture_output=True, text=True,
                          timeout=timeout, env=merged)


class EvidenceError(RuntimeError):
    pass


def extra_allow_zero(extra: dict | None) -> bool:
    """显式零案例豁免（当前 R09 全部套件不得使用；保留类型供未来清理型套件）。"""
    if not extra:
        return False
    return bool(extra.get("allowZeroCases")) and bool(str(extra.get("zeroCaseReason") or "").strip())


# cargo test 汇总行解析：`test result: ok. 12 passed; 0 failed; 1 ignored; ...`
_CARGO_SUMMARY = re.compile(
    r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored")


def parse_cargo_test_summaries(log: str) -> list[dict]:
    return [
        {"passed": int(match.group(1)), "failed": int(match.group(2)),
         "ignored": int(match.group(3))}
        for match in _CARGO_SUMMARY.finditer(log)
    ]


def require_no_skips(log: str, label: str = "cargo") -> dict:
    """关键 skip 保护：ignored/skipped 不得当作通过（R09 生产者统一调用）。

    返回汇总；任何 ignored>0 或没有解析到汇总行都抛 EvidenceError（零匹配）。
    """
    summaries = parse_cargo_test_summaries(log)
    if not summaries:
        raise EvidenceError(f"{label}: 未解析到 cargo test 汇总行（零匹配/输出被截断不可当通过）")
    ignored = sum(row["ignored"] for row in summaries)
    failed = sum(row["failed"] for row in summaries)
    if ignored:
        raise EvidenceError(f"{label}: 存在 ignored/skipped 测试（{ignored}），不得当通过")
    if failed:
        raise EvidenceError(f"{label}: 存在 failed 测试（{failed}）")
    return {"summaries": len(summaries), "passed": sum(row["passed"] for row in summaries)}


class Producer:
    """一个套件的生产者上下文：隔离运行时 + 回执 + 案例文件。"""

    def __init__(self, suite: str, evidence: Path):
        self.suite = suite
        self.evidence = Path(evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.started = time.time()
        stamp = f"{suite}-{os.getpid()}-{int(self.started * 1000)}"
        self.runtime = RUNTIME_ROOT / stamp
        self.home = self.runtime / "home"
        self.tmp = self.runtime / "tmp"
        self.raw = self.runtime / "raw"
        for path in (self.home, self.tmp, self.raw):
            path.mkdir(parents=True, exist_ok=True)
        self.logs: list[str] = []
        self.cases: list[dict] = []
        self.commands: list[dict] = []
        self.real_home_before = self._real_home_snapshot()

    # ── 隔离/数据安全 ────────────────────────────────────────────────────
    def env(self, extra: dict | None = None) -> dict:
        env = {
            "LINGXI_HOME": str(self.home),
            "TMPDIR": str(self.tmp),
            "TMP": str(self.tmp),
            "TEMP": str(self.tmp),
            "LINGXI_PRODUCT_DIR": str(ROOT / "lib"),
            "LINGXI_R09_EVIDENCE": str(self.evidence),
            "LINGXI_R09_RAW": str(self.raw),
        }
        env.update(extra or {})
        return env

    def _real_home_snapshot(self) -> dict:
        home = Path.home() / ".lingxi"
        if not home.is_dir():
            return {"exists": False, "digest": None}
        digest = hashlib.sha256()
        count = 0
        for path in sorted(home.rglob("*")):
            if not path.is_file():
                continue
            relative = path.relative_to(home)
            digest.update(str(relative).encode())
            digest.update(b"\0")
            digest.update(str(path.stat().st_size).encode())
            digest.update(b"\0")
            count += 1
        return {"exists": True, "digest": digest.hexdigest(), "fileCount": count}

    def assert_real_home_untouched(self) -> dict:
        after = self._real_home_snapshot()
        untouched = after == self.real_home_before
        self.check("isolation::real-home-untouched", untouched, 1 if untouched else 0,
                   detail={"before": self.real_home_before, "after": after})
        return after

    # ── 命令执行 ─────────────────────────────────────────────────────────
    def run_step(self, step_id: str, argv: list[str], timeout: int = 1800,
                 env: dict | None = None, expect_zero: bool = True) -> subprocess.CompletedProcess:
        started = time.time()
        process = run(argv, timeout=timeout, env=self.env(env))
        duration = time.time() - started
        log = f"$ {' '.join(argv)}\n--- exit={process.returncode} duration={duration:.2f}s ---\n"
        log += process.stdout + process.stderr
        self.logs.append(log)
        self.commands.append({
            "id": step_id, "argv": argv, "exitCode": process.returncode,
            "durationSecs": round(duration, 3),
            "logSha256": sha256_bytes(log.encode()),
        })
        if expect_zero and process.returncode != 0:
            raise EvidenceError(f"{step_id} failed (exit {process.returncode}): "
                                f"{(process.stdout + process.stderr)[-2000:]}")
        return process

    # ── 案例记录（xtask 逐断言合同） ─────────────────────────────────────
    def check(self, case: str, ok: bool, actual: int = 1, detail=None) -> bool:
        # 叶项契约（stage map assertionContract）要求证据文件每条案例同时记录
        # ok/actual/expect，且 actual==expect；check 的语义就是“该案例期望成立”，
        # 故 expect 恒为 1，失败时 actual=0 由门禁抓出。
        self.cases.append({
            "case": case,
            "ok": bool(ok),
            "actual": actual if ok else 0,
            "expect": 1,
            "detail": detail,
        })
        return bool(ok)

    def require(self, case: str, ok: bool, detail=None) -> None:
        if not self.check(case, ok, 1, detail):
            raise EvidenceError(f"case {case} failed: {detail}")

    # ── 回执输出 ─────────────────────────────────────────────────────────
    def finish(self, status: str = "PASS", extra: dict | None = None,
               cases_file: str = "cases.json") -> int:
        """写案例文件 + 回执；status=PASS 但存在失败 case 时强制 FAIL。

        R09 零匹配保护：一个“成功但一个案例都没有”的套件是假绿通道（门只能
        看到退出码和声明的证据文件，看不到“它其实什么都没验”）。因此 cases
        为空时一律 FAIL；只有显式声明 `allow_zero_cases=True` 且理由非空才放行
        （当前所有 R09 套件都必须有真实案例，不接受该豁免）。
        """
        if not self.cases and not extra_allow_zero(extra):
            status = "FAIL"
            extra = {**(extra or {}),
                     "zeroCaseGuard": "套件没有产生任何真实案例（零匹配 = 假绿，门必须红）"}
        failed = [case["case"] for case in self.cases if not case["ok"]]
        if status == "PASS" and failed:
            status = "FAIL"
            extra = {**(extra or {}), "failedCases": failed}
        cases_payload = {
            "schema": "lingxi.leaf-case-results.v1",
            "suite": self.suite,
            "cases": self.cases,
        }
        if self.cases:
            (self.evidence / cases_file).write_text(
                json.dumps(cases_payload, ensure_ascii=False, indent=1) + "\n",
                encoding="utf-8")
        log_text = "\n".join(self.logs)
        (self.evidence / "output.log").write_text(log_text, encoding="utf-8")
        if self.cases:
            self.check("suite::all-cases-hold", not failed, 1 if not failed else 0,
                       detail={"total": len(self.cases), "failed": failed})
        exit_code = 0 if status == "PASS" else 2
        receipt = {
            "schema": "lingxi.r09.suite-receipt.v1",
            "suite": self.suite,
            "status": status,
            "exitCode": exit_code,
            "durationSecs": round(time.time() - self.started, 3),
            "caseCount": len(self.cases),
            "casesSha256": (sha256_file(self.evidence / cases_file)
                            if self.cases else None),
            "commands": self.commands,
            "logSha256": sha256_bytes(log_text.encode()),
            "finishedAtUnixMs": int(time.time() * 1000),
        }
        if extra:
            receipt.update(extra)
        (self.evidence / "result.json").write_text(
            json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"suite {self.suite}: status={status} exit={exit_code} "
              f"cases={len(self.cases)} failed={failed}")
        return exit_code

    def fail(self, detail: str) -> int:
        """生产者前置条件失败：写回执并真实失败（门红）。"""
        print(f"suite {self.suite}: FAIL {detail}", flush=True)
        return self.finish(status="FAIL", extra={"failureDetail": detail})
