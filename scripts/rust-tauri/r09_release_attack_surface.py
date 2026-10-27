#!/usr/bin/env python3
"""R09-T06 A12：release 发布攻击面静态扫描（只读；绝不启动 app/GUI）。

只针对**已构建**的 release 产物与构建配置做只读取证：
  ① 无 wdio/webdriver/测试插件符号或字符串（host + sidecar）；
  ② 无 devUrl / Vite dev 端口（5173）嵌入产物或 release 配置；
  ③ 无 devtools/调试控制面开关（Cargo feature、源码 toggle 调用、ACL 权限 grant）；
  ④ sidecar 为固定二进制：externalBin 布局、Mach-O/arch、build.json
     protocol/sha256/contentSha256 校验（ad-hoc 签名后 sha 变化必须由
     contentSha256 证明内容未变），且无泛化 shell 入口（无 tauri-plugin-shell、
     capability 无 shell/http/fs 权限、process spawn 调用点与登记账本一致）；
  ⑤ capability 不含 debug 命令 / 插件泛化权限（静态 capabilities 文本）；
  ⑥ 测试 canary/调试命令名不出现在 release ACL 与二进制（从 build.rs +
     DEBUG_INVOKE_COMMANDS + policy.releaseDeniedCommands 提取候选名）。

证据：`result.json`（lingxi.r09.suite-receipt.v1）、`cases.json`
（lingxi.leaf-case-results.v1）、`output.log`、`scan-details.json`（前后
tree sha256、ACL 提取、CSP 解析、sidecar 摘要）。

退出码：0 PASS；2 FAIL；3 PENDING_POLICY（策略缺失）。
扫描前记录产物 tree sha256，扫描后复算：若构建期间产物变化，case
`release::artifact-stable-during-scan` 为红并在 result 标注
`artifactChangedDuringScan=true`——如实标注候选，不重试伪装稳定。

用法：
  python3 -B scripts/rust-tauri/r09_release_attack_surface.py --evidence <dir>
  python3 -B scripts/rust-tauri/r09_release_attack_surface.py --evidence <dir> \
      --app desktop/src-tauri/target/release/bundle/macos/Lingxi.app
"""

from __future__ import annotations

import hashlib
import json
import plistlib
import re
import struct
import subprocess
import sys
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
TAURI = ROOT / "desktop/src-tauri"
DEFAULT_APP = TAURI / "target/release/bundle/macos/Lingxi.app"
POLICY_PATH = ROOT / "docs/rust-tauri/R09/R09_COMMAND_POLICY.json"
CASES: list[dict] = []
LOGS: list[str] = []

SKIP_DIRS = {"target", "gen", "node_modules"}


# ── 基础设施 ────────────────────────────────────────────────────────────


def case(name: str, ok: bool, detail=None) -> bool:
    CASES.append({"case": name, "ok": bool(ok), "actual": 1 if ok else 0, "detail": detail})
    return bool(ok)


def log(message: str) -> None:
    print(message, flush=True)
    LOGS.append(message)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.is_file() else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(root: Path) -> dict:
    entries = {}
    if not root.is_dir():
        return {"exists": False, "treeSha256": None, "files": {}}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        entries[str(path.relative_to(root))] = {
            "sha256": sha256_file(path),
            "size": path.stat().st_size,
        }
    tree = hashlib.sha256()
    for name in sorted(entries):
        tree.update(name.encode())
        tree.update(b"\0")
        tree.update(entries[name]["sha256"].encode())
        tree.update(b"\0")
    return {"exists": True, "treeSha256": tree.hexdigest(), "files": entries}


def token_count(data: bytes, token: str, case_insensitive: bool) -> int:
    needle = token.encode()
    if case_insensitive:
        return data.lower().count(needle.lower())
    return data.count(needle)


def strip_rust_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def strip_rust_cfg_test(text: str) -> str:
    pattern = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
    while True:
        match = pattern.search(text)
        if not match:
            return text
        brace = text.find("{", match.end())
        if brace < 0:
            text = text[: match.start()]
            continue
        depth = 0
        end = len(text)
        for index in range(brace, len(text)):
            char = text[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    end = index + 1
                    break
        text = text[: match.start()] + text[end:]


def run(argv: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=ROOT, capture_output=True, text=True,
                          timeout=timeout, env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin"})


# ── Mach-O / build.json ─────────────────────────────────────────────────


def macho_cputype(data: bytes) -> tuple[int, int] | None:
    if len(data) < 8 or struct.unpack_from("<I", data, 0)[0] != 0xFEEDFACF:
        return None
    return struct.unpack_from("<I", data, 4)[0], struct.unpack_from("<I", data, 12)[0]


def macho_content_digest(data: bytes) -> str | None:
    """与 desktop/src/shared/rust-local-service.cjs 的 mac content digest 同算法。

    签名块可被 ad-hoc 签名重写；把 LC_CODE_SIGNATURE 的长度字段与
    __LINKEDIT 的 fileoff/filesize 三处清零后对签名前内容做 sha256。
    """
    if len(data) < 32 or struct.unpack_from("<I", data, 0)[0] != 0xFEEDFACF:
        return None
    ncmds = struct.unpack_from("<I", data, 16)[0]
    sizeofcmds = struct.unpack_from("<I", data, 20)[0]
    commands_end = 32 + sizeofcmds
    if ncmds > 512 or commands_end > len(data):
        return None
    offset = 32
    signature = None
    linkedit = None
    for _ in range(ncmds):
        if offset + 8 > commands_end:
            return None
        command, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > commands_end:
            return None
        if command == 0x1D:  # LC_CODE_SIGNATURE
            signature = {
                "offset": offset,
                "dataOffset": struct.unpack_from("<I", data, offset + 8)[0],
                "size": struct.unpack_from("<I", data, offset + 12)[0],
            }
        if command == 0x19:  # LC_SEGMENT_64
            name = data[offset + 8:offset + 24].split(b"\0", 1)[0]
            if name == b"__LINKEDIT":
                linkedit = {
                    "offset": offset,
                    "fileOffset": struct.unpack_from("<Q", data, offset + 40)[0],
                    "fileSize": struct.unpack_from("<Q", data, offset + 48)[0],
                }
        offset += size
    if offset != commands_end:
        return None
    if signature is None:
        return hashlib.sha256(data).hexdigest()
    if (linkedit is None or signature["size"] < 1
            or signature["dataOffset"] + signature["size"] != len(data)
            or linkedit["fileOffset"] > signature["dataOffset"]
            or linkedit["fileOffset"] + linkedit["fileSize"] != len(data)):
        return None
    content = bytearray(data[: signature["dataOffset"]])
    struct.pack_into("<Q", content, linkedit["offset"] + 32, 0)
    struct.pack_into("<Q", content, linkedit["offset"] + 48, 0)
    struct.pack_into("<I", content, signature["offset"] + 12, 0)
    return hashlib.sha256(bytes(content)).hexdigest()


# ── release 配置 / ACL / CSP ────────────────────────────────────────────


def load_release_config() -> tuple[dict | None, str]:
    generated = TAURI / "tauri.release.generated.conf.json"
    plain = TAURI / "tauri.release.conf.json"
    for path in (generated, plain):
        if path.is_file():
            try:
                return json.loads(read(path)), str(path.relative_to(ROOT))
            except json.JSONDecodeError:
                continue
    return None, "missing"


def parse_csp(csp: str) -> dict[str, list[str]]:
    directives: dict[str, list[str]] = {}
    for part in (csp or "").split(";"):
        tokens = part.strip().split()
        if not tokens:
            continue
        directives[tokens[0].lower()] = tokens[1:]
    return directives


def extract_acl_region(data: bytes) -> bytes | None:
    """提取 tauri-build 嵌入的 ACL 清单区域。

    ACL 清单不是单段字符串：`__app-acl__` 标记段之后/之前还有分段的
    printable run（命令名、权限标识、plugin 权限、内嵌 config 片段），中间隔着
    二进制数据。取标记前后窗口内所有 ACL 相关 printable run 拼接后再解析。
    """
    marker = b"__app-acl__"
    start = data.find(marker)
    if start < 0:
        return None
    lo = max(0, start - 2_500_000)
    hi = min(len(data), start + 400_000)
    runs = re.findall(rb"[ -~]{8,}", data[lo:hi])
    acl_runs = [run for run in runs
                if any(token in run for token in
                       (b"allow-", b"deny-", b"core:", b"__app-acl__"))]
    return b"\n".join(acl_runs) if acl_runs else None


def registered_release_commands() -> set[str]:
    """当前生产注册面（build.rs manifest ∪ invoke_handler ∪ native INVOKE_COMMANDS）。"""
    commands: set[str] = set()
    build = strip_rust_comments(read(TAURI / "build.rs"))
    match = re.search(r"AppManifest::new\(\)\s*\.commands\(\s*&?\[(.*?)\]", build, re.S)
    if match:
        commands.update(re.findall(r'"([A-Za-z0-9_\-]+)"', match.group(1)))
    else:
        # D11：build.rs 用 Vec 构造命令面（按 feature 追加 debug 命令）后再传
        # 引用，无法用内联数组正则解析；提取全部 snake_case 字符串字面量。
        commands.update(re.findall(r'"([a-z][a-z0-9_]{3,})"', build))
    lib = strip_rust_comments(read(TAURI / "src/lib.rs"))
    for item in re.findall(r"generate_handler!\[(.*?)\]", lib, re.S):
        for token in item.split(","):
            name = token.strip().split("::")[-1].strip()
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
                commands.add(name)
    install = strip_rust_comments(read(TAURI / "src/native/install.rs"))
    for const in ("INVOKE_COMMANDS", "DEBUG_INVOKE_COMMANDS"):
        match = re.search(rf"{const}[^=]*=\s*&?\[(.*?)\];", install, re.S)
        if match:
            commands.update(re.findall(r'"([A-Za-z0-9_\-]+)"', match.group(1)))
    return commands


def strong_command_in(segment: bytes, command: str) -> bool:
    """命令是否可信地出现在 ACL 中：`allow-kebab<snake>` 配对或独立 token。"""
    kebab = command.replace("_", "-").encode()
    if b"allow-" + kebab + command.encode() in segment:
        return True
    pattern = rb"(?<![a-z0-9_])" + re.escape(command.encode()) + rb"(?![a-z0-9_])"
    return re.search(pattern, segment) is not None


def acl_command_candidates(policy: dict) -> set[str]:
    """候选命令名 = 策略面 ∪ dev/deferred/dropped ∪ 当前注册面 ∪ ACL 生成清单。"""
    candidates: set[str] = set()
    for spec in (policy.get("windows") or {}).values():
        candidates.update(spec.get("allowedCommands") or [])
    candidates.update(policy.get("devOnlyCommands") or [])
    candidates.update(policy.get("releaseDeniedCommands") or [])
    candidates.update((policy.get("droppedCommands") or {}).keys())
    for group in (policy.get("deferredCommands") or {}).values():
        candidates.update(group.get("commands") or [])
    build = read(TAURI / "build.rs")
    match = re.search(r"AppManifest::new\(\)\s*\.commands\(\s*&?\[(.*?)\]", build, re.S)
    if match:
        candidates.update(re.findall(r'"([A-Za-z0-9_\-]+)"', match.group(1)))
    lib = strip_rust_comments(read(TAURI / "src/lib.rs"))
    for item in re.findall(r"generate_handler!\[(.*?)\]", lib, re.S):
        for token in item.split(","):
            name = token.strip().split("::")[-1].strip()
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
                candidates.add(name)
    install = strip_rust_comments(read(TAURI / "src/native/install.rs"))
    for const in ("INVOKE_COMMANDS", "DEBUG_INVOKE_COMMANDS"):
        match = re.search(rf"{const}[^=]*=\s*&?\[(.*?)\];", install, re.S)
        if match:
            candidates.update(re.findall(r'"([A-Za-z0-9_\-]+)"', match.group(1)))
    return candidates


def acl_commands(segment: bytes, candidates: set[str], registered: set[str]) -> list[str]:
    """候选命令是否进入 release ACL。

    序列化格式对一部分条目是 `<allow-kebab><snake>`，另一部分是分段后的裸命令名
    列表。裸名只在“当前注册面”内接受（避免如 `close_browser` 是
    `close_browser_viewer` 子串的误报）；未注册候选只接受配对或独立 token。
    """
    found = []
    for command in sorted(candidates):
        if strong_command_in(segment, command):
            found.append(command)
        elif command in registered and command.encode() in segment:
            found.append(command)
    return found


# ── 扫描主体 ────────────────────────────────────────────────────────────


def scan(evidence: Path, policy: dict, policy_path: Path, app: Path) -> int:
    started = time.time()
    baseline = policy.get("securityBaseline") or {}
    artifact = baseline.get("releaseArtifact") or {}
    case("release::artifact-present", app.is_dir(), {"app": str(app)})
    if not app.is_dir():
        case("release::artifact-stable-during-scan", False,
             {"reason": "release 产物不存在，无法扫描"})
        return finish(evidence, started, policy_path, app, None, None, {},
                      {"blocked": "RELEASE_ARTIFACT_MISSING"})

    before = tree_digest(app)
    host_rel = artifact.get("hostBinary") or "Contents/MacOS/lingxi-resource-desktop"
    sidecar_rel = artifact.get("sidecarBinary") or "Contents/MacOS/lingxi-service"
    manifest_rel = artifact.get("sidecarManifest") or "Contents/Resources/rust-service/build.json"
    host_path = app / host_rel
    sidecar_path = app / sidecar_rel
    manifest_path = app / manifest_rel
    host_bytes = host_path.read_bytes() if host_path.is_file() else b""
    sidecar_bytes = sidecar_path.read_bytes() if sidecar_path.is_file() else b""

    details: dict = {
        "app": str(app.relative_to(ROOT)) if str(app).startswith(str(ROOT)) else str(app),
        "artifactBefore": before,
        "host": {"path": host_rel, "size": len(host_bytes),
                 "sha256": hashlib.sha256(host_bytes).hexdigest()},
        "sidecar": {"path": sidecar_rel, "size": len(sidecar_bytes),
                    "sha256": hashlib.sha256(sidecar_bytes).hexdigest()},
        "manifest": None,
        "releaseConfig": None,
        "csp": None,
        "acl": None,
        "codesign": None,
        "spawnSites": None,
        "forbiddenStringHits": [],
    }

    # ── ① 测试控制面符号 ────────────────────────────────────────────
    forbidden = baseline.get("forbiddenArtifactStrings") or []
    hits = []
    for item in forbidden:
        token = item["token"]
        for name, data in (("host", host_bytes), ("sidecar", sidecar_bytes)):
            count = token_count(data, token, bool(item.get("caseInsensitive")))
            if count:
                hits.append({"artifact": name, "token": token, "count": count,
                             "reason": item.get("reason")})
    details["forbiddenStringHits"] = hits
    wdio_hits = [h for h in hits if h["token"].lower() in ("wdio", "webdriver", "tauri-plugin-wdio")]
    case("release::no-wdio-webdriver-symbols", not wdio_hits, wdio_hits)
    dev_server_hits = [h for h in hits if "5173" in h["token"]]
    case("release::no-dev-server-url-in-artifacts", not dev_server_hits, dev_server_hits)
    devurl_hits = [h for h in hits if h["token"].lower() == "devurl"]
    case("release::no-devurl-in-artifacts", not devurl_hits, devurl_hits)
    debug_hits = [h for h in hits if h["token"].startswith("debug_")]
    case("release::no-debug-command-names-in-artifacts", not debug_hits, debug_hits)

    # ── ② release 配置 ──────────────────────────────────────────────
    config, config_path = load_release_config()
    details["releaseConfig"] = {"path": config_path,
                                "withGlobalTauri": ((config or {}).get("app") or {}).get("withGlobalTauri"),
                                "frontendDist": ((config or {}).get("build") or {}).get("frontendDist"),
                                "devUrl": ((config or {}).get("build") or {}).get("devUrl"),
                                "identifier": (config or {}).get("identifier"),
                                "version": (config or {}).get("version"),
                                "bundle": (config or {}).get("bundle")}
    case("release::release-config-present", config is not None, config_path)
    expected_global = (baseline.get("releaseConfig") or {}).get("withGlobalTauri")
    actual_global = ((config or {}).get("app") or {}).get("withGlobalTauri")
    case("release::withglobaltauri-matches-policy", actual_global == expected_global,
         {"actual": actual_global, "policy": expected_global,
          "note": "withGlobalTauri 只暴露 ACL 允许的命令，不绕过 ACL；R09 保留 true 以复用同一 React 产物"})
    expected_dist = (baseline.get("releaseConfig") or {}).get("frontendDist")
    actual_dist = ((config or {}).get("build") or {}).get("frontendDist")
    case("release::frontend-dist-is-product-build", actual_dist == expected_dist,
         {"actual": actual_dist, "expected": expected_dist})
    case("release::release-config-no-devurl", not ((config or {}).get("build") or {}).get("devUrl"),
         {"devUrl": ((config or {}).get("build") or {}).get("devUrl")})
    remote_ipc = (((config or {}).get("app") or {}).get("security") or {}).get(
        "dangerousRemoteDomainIpcAccess")
    case("release::no-remote-domain-ipc-access", not remote_ipc,
         {"dangerousRemoteDomainIpcAccess": remote_ipc,
          "note": "远程内容 webview 不得获 Tauri IPC；未配置 = 默认拒绝"})
    bundle = (config or {}).get("bundle") or {}
    expected_external = (((baseline.get("sidecar") or {}).get("externalBin")) or [])
    case("release::bundle-external-bin", (bundle.get("externalBin") or []) == expected_external,
         {"actual": bundle.get("externalBin"), "expected": expected_external})
    macos = bundle.get("macOS") or {}
    case("release::signing-identity-declared",
         macos.get("signingIdentity") == "-",
         {"signingIdentity": macos.get("signingIdentity"),
          "note": "ad-hoc '-'（BLOCKED-SIGNING-IDENTITY，不冒充 Developer ID/公证）"})

    # 二进制内嵌配置：必须指向产品 renderer，不含 dev 产物。
    embedded_ok = (b"../dist-renderer" in host_bytes
                   and b"../dist-tauri-resources" not in host_bytes)
    case("release::embedded-frontend-dist-product", embedded_ok,
         {"distRenderer": b"../dist-renderer" in host_bytes,
          "distTauriResources": b"../dist-tauri-resources" in host_bytes})

    # ── ③ CSP ───────────────────────────────────────────────────────
    csp_text = (((config or {}).get("app") or {}).get("security") or {}).get("csp") or ""
    directives = parse_csp(csp_text)
    details["csp"] = {"text": csp_text, "directives": directives}
    forbidden_csp = [token for token in
                     ((baseline.get("releaseConfig") or {}).get("cspForbiddenSubstrings") or [])
                     if token.lower() in csp_text.lower()]
    case("release::csp-no-forbidden-tokens", not forbidden_csp, forbidden_csp)
    allowed_sources = (baseline.get("releaseConfig") or {}).get("cspAllowedSources") or {}
    csp_errors = {}
    for directive, required in allowed_sources.items():
        actual_tokens = directives.get(directive)
        if actual_tokens is None:
            csp_errors[directive] = {"missingDirective": True}
            continue
        required_set = set(required)
        actual_set = set(actual_tokens)
        extra = sorted(actual_set - required_set)
        missing = sorted(required_set - actual_set)
        if extra or missing:
            csp_errors[directive] = {"extraSources": extra, "missingRequired": missing}
    case("release::csp-directives-hardened", not csp_errors, csp_errors)

    # ── ③/⑥ ACL：release 命令面 & devtools/插件泛化权限 ─────────────
    segment = extract_acl_region(host_bytes)
    case("release::acl-segment-extracted", segment is not None,
         {"bytes": len(segment) if segment else 0})
    release_commands: list[str] = []
    registered = registered_release_commands()
    if segment is not None:
        candidates = acl_command_candidates(policy)
        release_commands = acl_commands(segment, candidates, registered)
        details["acl"] = {
            "segmentBytes": len(segment),
            "candidateCount": len(candidates),
            "releaseCommands": release_commands,
            "commandCount": len(release_commands),
            "pluginPermissionIdentifiers": sorted({
                item.decode("utf-8", "replace") for item in re.findall(
                    rb"(?:core:[a-z0-9:_-]+|[a-z][a-z0-9_-]*:allow-[a-z0-9-]+)",
                    segment)}),
        }
    case("release::acl-command-surface-nonempty", bool(release_commands),
         {"count": len(release_commands)})
    surface = set()
    for spec in (policy.get("windows") or {}).values():
        surface.update(spec.get("allowedCommands") or [])
    undeclared = sorted(set(release_commands) - surface)
    case("release::acl-command-surface-subset-of-policy", not undeclared, undeclared)
    release_denied = sorted(command for command in policy.get("releaseDeniedCommands") or []
                            if segment is not None and strong_command_in(segment, command))
    case("release::acl-no-release-denied-commands", not release_denied, release_denied)
    deferred: set[str] = set()
    for group in (policy.get("deferredCommands") or {}).values():
        deferred.update(group.get("commands") or [])
    deferred_hits = sorted(command for command in deferred
                           if segment is not None and strong_command_in(segment, command))
    case("release::acl-no-deferred-commands", not deferred_hits, deferred_hits)
    dropped_hits = sorted(command for command in (policy.get("droppedCommands") or {}).keys()
                          if segment is not None and strong_command_in(segment, command))
    case("release::acl-no-dropped-commands", not dropped_hits, dropped_hits)
    forbidden_patterns = policy.get("forbiddenCommandPatterns") or []
    forbidden_command_hits = sorted(command for command in release_commands
                                    if any(re.search(pattern, command) for pattern in forbidden_patterns))
    case("release::acl-no-forbidden-command-patterns", not forbidden_command_hits,
         forbidden_command_hits)

    if segment is not None:
        # 只匹配“插件权限标识”形态，避免把内嵌 CSP 里的 http:// 误判成 http 插件。
        generic_pattern = rb"(?:shell|http|https|fs|process|updater|dialog|notification|global-shortcut|autostart):(?:allow-|deny-|default)"
        plugin_hits = {
            "shell": len(re.findall(rb"shell:(?:allow-|deny-|default)", segment)),
            "http": len(re.findall(rb"https?:(?:allow-|deny-|default)", segment)),
            "fs": len(re.findall(rb"fs:(?:allow-|deny-|default)", segment)),
            "process": len(re.findall(rb"process:(?:allow-|deny-|default)", segment)),
            "updater": len(re.findall(rb"updater:(?:allow-|deny-|default)", segment)),
            "devtoolsPermission": segment.count(b"internal-toggle-devtools"),
            "trayPermission": segment.count(b"core:tray:default"),
            "menuPermission": segment.count(b"core:menu:default"),
            "genericPluginIds": len(re.findall(generic_pattern, segment)),
        }
        dangerous = {key: value for key, value in plugin_hits.items()
                     if key in ("shell", "http", "fs", "process", "updater") and value}
        case("release::acl-no-shell-http-fs-process-updater-grants", not dangerous, plugin_hits)
        # 说明：Tauri 会在 ACL 定义数据里内嵌 core 插件的 permission *定义*
        # （core:webview:default 字段里含 internal-toggle-devtools 等标识符），
        # 该字符串存在不等于 capability 授权。devtools/tray/menu 的**授权面**
        # 改由下面的 capability 静态展开检查（grant 集合），避免把定义误判成 grant。

    # ── ③ devtools 开关（Cargo feature / 源码） ────────────────────
    cargo = read(TAURI / "Cargo.toml")
    tauri_dep = re.search(r"^tauri\s*=\s*\{([^}]*)\}", cargo, re.M)
    tauri_features = re.findall(r'"([a-z0-9\-]+)"', tauri_dep.group(1)) if tauri_dep else []
    devtools_cfg = (baseline.get("devtools") or {})
    forbidden_features = [f for f in devtools_cfg.get("cargoFeaturesForbidden") or []
                          if f in tauri_features]
    case("release::cargo-devtools-feature-off", not forbidden_features,
         {"tauriFeatures": tauri_features, "forbiddenEnabled": forbidden_features})
    source_hits = []
    for path in sorted((TAURI / "src").glob("**/*.rs")):
        body = strip_rust_cfg_test(strip_rust_comments(read(path)))
        for pattern in devtools_cfg.get("sourceForbiddenPatterns") or []:
            if pattern in body:
                source_hits.append({"file": str(path.relative_to(ROOT)), "pattern": pattern})
    case("release::source-no-devtools-toggle", not source_hits, source_hits)

    # ── ⑥ 测试控制面 env / 能力清单静态文本 ────────────────────────
    test_env = (policy.get("testControl") or {}).get("releaseMustReject") or []
    env_hits = []
    for env_name in test_env:
        for name, data in (("host", host_bytes), ("sidecar", sidecar_bytes)):
            count = data.count(env_name.encode())
            if count:
                env_hits.append({"env": env_name, "artifact": name, "count": count})
    case("release::no-test-control-env-in-artifacts", not env_hits,
         {"hits": env_hits,
          "note": "release 二进制不应含仅测试用的 env 注入点（host.rs 自动化 sink）"})
    capability_hits = []
    for path in sorted((TAURI / "capabilities").glob("*.json")):
        text = read(path)
        for command in set(policy.get("releaseDeniedCommands") or []) | \
                set(policy.get("devOnlyCommands") or []):
            if command in text:
                capability_hits.append({"file": str(path.relative_to(ROOT)), "command": command})
    case("release::capabilities-no-debug-command-grants", not capability_hits, capability_hits)

    # ── ⑤/③ capability 授权面（grant，不是 ACL 定义）────────────────
    # release 构建实际使用的 capability 文件 = 本树 capabilities/*.json（构建回执的
    # crateSourceDigest 绑定其内容）；逐条检查插件权限是否是策略允许的最小 leaf。
    baseline_plugins = set((policy.get("pluginPermissionBaseline") or {}).get("leaves") or [])
    forbidden_plugin_prefixes = ("shell:", "http:", "https:", "fs:", "process:", "updater:",
                                 "dialog:", "notification:", "global-shortcut:", "autostart:",
                                 "core:default", "core:tray:", "core:menu:", "core:webview:",
                                 "devtools")
    capability_grant_files = {}
    illegal_grants = []
    for path in sorted((TAURI / "capabilities").glob("*.json")):
        try:
            cap = json.loads(read(path))
        except json.JSONDecodeError:
            illegal_grants.append({"file": str(path.relative_to(ROOT)), "error": "invalid-json"})
            continue
        permissions = cap.get("permissions") or []
        capability_grant_files[str(path.relative_to(ROOT))] = {
            "windows": cap.get("windows"),
            "permissionCount": len(permissions),
            "sha256": sha256_file(path),
        }
        for permission in permissions:
            if permission.startswith("allow-") or permission.startswith("deny-"):
                continue
            if permission in baseline_plugins:
                continue
            if any(permission.startswith(prefix) or prefix in permission
                   for prefix in forbidden_plugin_prefixes):
                illegal_grants.append({"file": str(path.relative_to(ROOT)),
                                       "permission": permission})
    case("release::capabilities-plugin-grants-minimal", not illegal_grants,
         {"allowedLeaves": sorted(baseline_plugins), "illegal": illegal_grants,
          "files": capability_grant_files})

    # ── ④ sidecar 固定二进制 ────────────────────────────────────────
    case("release::sidecar-binary-present",
         sidecar_path.is_file() and bool(sidecar_bytes), {"path": sidecar_rel})
    cputype = macho_cputype(sidecar_bytes) if sidecar_bytes else None
    expected_arch = {"arm64": 0x0100000C, "x64": 0x01000007}.get(artifact.get("arch") or "")
    case("release::sidecar-macho-format",
         cputype is not None and cputype[0] == expected_arch,
         {"magic": "feedfacf" if cputype else None,
          "cputype": hex(cputype[0]) if cputype else None,
          "expectedCputype": hex(expected_arch) if expected_arch else None})
    case("release::sidecar-not-shell-script",
         bool(sidecar_bytes) and not sidecar_bytes.startswith(b"#!"),
         {"firstBytes": sidecar_bytes[:16].hex() if sidecar_bytes else None})
    try:
        build_manifest = json.loads(read(manifest_path)) if manifest_path.is_file() else None
    except json.JSONDecodeError:
        build_manifest = None
    details["manifest"] = build_manifest
    required_fields = (baseline.get("sidecar") or {}).get("requiredManifestFields") or []
    missing_fields = [field for field in required_fields
                      if build_manifest is None or field not in build_manifest]
    case("release::sidecar-build-manifest-fields", not missing_fields,
         {"path": manifest_rel, "missingFields": missing_fields})
    expected_protocol = (baseline.get("sidecar") or {}).get("requiredProtocol") or {}
    actual_protocol = (build_manifest or {}).get("protocol")
    case("release::sidecar-protocol-identity", actual_protocol == expected_protocol,
         {"actual": actual_protocol, "expected": expected_protocol})
    sidecar_sha = hashlib.sha256(sidecar_bytes).hexdigest() if sidecar_bytes else None
    content_digest = macho_content_digest(sidecar_bytes) if sidecar_bytes else None
    manifest_sha = (build_manifest or {}).get("sha256")
    manifest_content = (build_manifest or {}).get("contentSha256")
    sha_match = sidecar_sha == manifest_sha
    content_match = content_digest == manifest_content
    case("release::sidecar-content-digest",
         bool(sidecar_bytes) and content_match,
         {"manifestSha256": manifest_sha, "actualSha256": sidecar_sha,
          "shaMatches": sha_match,
          "manifestContentSha256": manifest_content,
          "actualContentSha256": content_digest,
          "contentMatches": content_match,
          "note": "ad-hoc 签名会改 sha256；内容不变性由 contentSha256 证明"})
    identity_fields = {
        "platform": (build_manifest or {}).get("platform"),
        "arch": (build_manifest or {}).get("arch"),
        "target": (build_manifest or {}).get("target"),
        "binary": (build_manifest or {}).get("binary"),
    }
    case("release::sidecar-build-identity",
         identity_fields["platform"] == artifact.get("platformTag")
         and identity_fields["arch"] == artifact.get("arch")
         and identity_fields["target"] == artifact.get("targetTriple")
         and identity_fields["binary"] == sidecar_path.name,
         {"actual": identity_fields,
          "expected": {"platform": artifact.get("platformTag"),
                       "arch": artifact.get("arch"),
                       "target": artifact.get("targetTriple"),
                       "binary": sidecar_path.name}})

    # Info.plist / appVersion 绑定
    plist_path = app / "Contents/Info.plist"
    try:
        plist = plistlib.loads(plist_path.read_bytes()) if plist_path.is_file() else {}
    except Exception:  # noqa: BLE001 - 产物损坏必须显式红，不静默吞成 PASS
        plist = {}
    details["infoPlist"] = {key: plist.get(key) for key in
                            ("CFBundleIdentifier", "CFBundleExecutable",
                             "CFBundleShortVersionString", "NSMicrophoneUsageDescription",
                             "NSSpeechRecognitionUsageDescription")}
    case("release::bundle-identity",
         plist.get("CFBundleIdentifier") == "com.lingxi.app"
         and plist.get("CFBundleExecutable") == host_path.name,
         {"identifier": plist.get("CFBundleIdentifier"),
          "executable": plist.get("CFBundleExecutable"),
          "expectedExecutable": host_path.name})
    case("release::bundle-version-matches-manifest",
         build_manifest is not None
         and plist.get("CFBundleShortVersionString") == (build_manifest or {}).get("appVersion"),
         {"plistVersion": plist.get("CFBundleShortVersionString"),
          "manifestAppVersion": (build_manifest or {}).get("appVersion")})

    # ── ④ 无泛化 shell 入口 ─────────────────────────────────────────
    case("release::no-shell-plugin-dependency", "tauri-plugin-shell" not in cargo,
         {"cargoMentionsShellPlugin": "tauri-plugin-shell" in cargo})
    capability_shell = []
    for path in sorted((TAURI / "capabilities").glob("*.json")):
        text = read(path)
        for prefix in ("shell:", "http:", "fs:", "process:", "updater:"):
            if prefix in text:
                capability_shell.append({"file": str(path.relative_to(ROOT)), "prefix": prefix})
    case("release::capabilities-no-generic-shell-http-fs", not capability_shell, capability_shell)
    shell_baseline = baseline.get("shellSurface") or {}
    expected_sites = shell_baseline.get("expectedProcessSpawnSites") or {}
    actual_sites = {}
    spawn_samples = []
    for path in sorted((TAURI / "src").glob("**/*.rs")):
        body = strip_rust_cfg_test(strip_rust_comments(read(path)))
        count = body.count("Command::new(")
        if count:
            actual_sites[str(path.relative_to(ROOT))] = count
            spawn_samples.extend(re.findall(r"Command::new\(([^)]{0,80})\)", body))
    case("release::process-spawn-sites-match-ledger", actual_sites == expected_sites,
         {"actual": actual_sites, "expected": expected_sites, "samples": spawn_samples})
    forbidden_binaries = shell_baseline.get("forbiddenFixedBinaries") or []
    spawn_text = "\n".join(spawn_samples)
    fixed_hits = [binary for binary in forbidden_binaries if binary in spawn_text]
    case("release::no-renderer-controllable-shell-binary", not fixed_hits,
         {"hits": fixed_hits, "samples": spawn_samples})

    # ── 签名与 bundle 完整性 ────────────────────────────────────────
    verify = run(["/usr/bin/codesign", "--verify", "--strict", str(app)], timeout=120)
    display = run(["/usr/bin/codesign", "-dvvv", str(app)], timeout=120)
    signature = None
    for line in (display.stdout + display.stderr).splitlines():
        if line.startswith("Signature="):
            signature = line.split("=", 1)[1].strip()
    details["codesign"] = {"verifyExit": verify.returncode,
                           "signature": signature,
                           "display": (display.stdout + display.stderr)[-2000:]}
    case("release::codesign-verify-strict", verify.returncode == 0,
         {"exitCode": verify.returncode, "stderr": verify.stderr[-500:]})
    case("release::signing-identity-is-adhoc-as-documented", signature == "adhoc",
         {"signature": signature,
          "note": "BLOCKED-SIGNING-IDENTITY：本地 ad-hoc，不冒充正式分发签名/公证"})

    electron_files = [name for name in before["files"]
                      if "electron" in name.lower() or name.endswith(".asar")]
    case("release::no-electron-runtime-in-bundle", not electron_files, electron_files)

    # ── 产物稳定性（构建期间可能被 host-core 重建） ────────────────
    after = tree_digest(app)
    details["artifactAfter"] = after
    stable = before == after
    host_current = sha256_file(host_path) if host_path.is_file() else None
    details["artifactChangedDuringScan"] = not stable
    case("release::artifact-stable-during-scan", stable,
         {"beforeTreeSha256": before.get("treeSha256"),
          "afterTreeSha256": after.get("treeSha256"),
          "hostSha256Now": host_current,
          "hostSha256Scanned": details["host"]["sha256"],
          "note": "构建期产物变化时本证据只代表扫描到的候选版本，不重试伪装稳定"})

    return finish(evidence, started, policy_path, app, details, manifest_path, {},
                  {"candidate": True})


def finish(evidence: Path, started: float, policy_path: Path, app: Path,
           details: dict | None, manifest_path: Path | None,
           extra: dict, extra_top: dict) -> int:
    failed = [row["case"] for row in CASES if not row["ok"]]
    status = "PASS" if not failed else "FAIL"
    exit_code = 0 if not failed else 2
    result = {
        "schema": "lingxi.r09.release-attack-surface.v1",
        "suite": "r09_release_attack_surface",
        "status": status,
        "exitCode": exit_code,
        "policy": str(policy_path.relative_to(ROOT)) if policy_path.is_file() else None,
        "app": str(app.relative_to(ROOT)) if str(app).startswith(str(ROOT)) else str(app),
        "caseCount": len(CASES),
        "failedCases": failed,
        "artifactChangedDuringScan": bool((details or {}).get("artifactChangedDuringScan")),
        "durationSecs": round(time.time() - started, 3),
    }
    result.update(extra)
    result.update(extra_top)
    if details:
        result.update({key: details[key] for key in
                       ("artifactBefore", "artifactAfter", "host", "sidecar", "manifest",
                        "releaseConfig", "csp", "acl", "codesign", "forbiddenStringHits",
                        "infoPlist") if key in details})
    if evidence is not None:
        (evidence / "scan-details.json").write_text(
            json.dumps({"schema": "lingxi.r09.release-scan-details.v1", **result,
                        "cases": CASES}, ensure_ascii=False, indent=1) + "\n",
            encoding="utf-8")
        (evidence / "cases.json").write_text(json.dumps(
            {"schema": "lingxi.leaf-case-results.v1",
             "suite": "r09_release_attack_surface", "cases": CASES},
            ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        (evidence / "output.log").write_text("\n".join(LOGS) + "\n", encoding="utf-8")
        (evidence / "result.json").write_text(json.dumps({
            "schema": "lingxi.r09.suite-receipt.v1",
            "suite": "r09_release_attack_surface",
            "status": status,
            "exitCode": exit_code,
            "caseCount": len(CASES),
            "failedCases": failed,
            "artifactChangedDuringScan": result["artifactChangedDuringScan"],
            "details": "scan-details.json",
        }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"r09_release_attack_surface: status={status} cases={len(CASES)} failed={failed} "
          f"artifactChanged={result['artifactChangedDuringScan']}")
    return exit_code


def main() -> int:
    started = time.time()
    evidence = None
    if "--evidence" in sys.argv:
        evidence = Path(sys.argv[sys.argv.index("--evidence") + 1])
        evidence = evidence if evidence.is_absolute() else ROOT / evidence
        evidence.mkdir(parents=True, exist_ok=True)
    policy_path = (Path(sys.argv[sys.argv.index("--policy") + 1])
                   if "--policy" in sys.argv else POLICY_PATH)
    policy_path = policy_path if policy_path.is_absolute() else ROOT / policy_path
    app = (Path(sys.argv[sys.argv.index("--app") + 1])
           if "--app" in sys.argv else DEFAULT_APP)
    app = app if app.is_absolute() else ROOT / app
    if not policy_path.is_file():
        log(f"policy 缺失：{policy_path}；A12 扫描需要策略基线（PENDING_POLICY）")
        if evidence is not None:
            (evidence / "result.json").write_text(json.dumps({
                "schema": "lingxi.r09.suite-receipt.v1",
                "suite": "r09_release_attack_surface",
                "status": "PENDING_POLICY", "exitCode": 3,
                "policy": str(policy_path),
            }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
            (evidence / "output.log").write_text("\n".join(LOGS) + "\n", encoding="utf-8")
        return 3
    policy = json.loads(read(policy_path))
    try:
        return scan(evidence, policy, policy_path, app)
    except Exception as error:  # noqa: BLE001 - 扫描器自身失败必须红，不静默吞
        CASES.append({"case": "release::scanner-exception", "ok": False, "actual": 0,
                      "detail": {"error": f"{type(error).__name__}: {error}"}})
        return finish(evidence, started, policy_path, app, None, None, {},
                      {"scannerError": f"{type(error).__name__}: {error}"})


if __name__ == "__main__":
    sys.exit(main())
