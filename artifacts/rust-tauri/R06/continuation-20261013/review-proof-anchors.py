"""Check recorded assertion text against current tests without changing verdicts."""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
MAP = ROOT / "scripts/rust-tauri/r06_original_leaf_bindings.json"
REPORT = Path(__file__).with_name("proof-anchor-review-final-format.json")
TOKEN = re.compile(
    r'//[^\n]*|/\*.*?\*/|r(#+)".*?"\1|"(?:\\.|[^"\\])*"|'
    r"'(?:\\.|[^'\\])'|\s+|[A-Za-z_]\w*|\d[\w.]*|.",
    re.DOTALL,
)
REVIEWED_CHANGED_TESTS = {
    "subagent_close_and_reply_refusals_preserve_other_threads_and_sessions",
    "websocket_registration_and_foreign_messages_preserve_authorized_session_streams",
    "stage_files_gateway_only_delivers_real_authorized_session_files",
    "original_archive_children_recurses_skips_running_and_preserves_failed_transaction",
    "original_session_search_returns_scoped_title_content_and_explicit_read_failures",
    "resourceio_write_conflict_refuses_external_change_at_commit_boundary",
    "file_history_failures_and_conflict_preserve_user_bytes_and_snapshot_rows",
    "file_history_routes_full_chain_and_error_vocabulary",
    "original_persona_reads_template_custom_alias_and_refusal",
    "original_manage_refusals_and_no_write_authorization_preserve_facts",
    "a13_search_within_scope_returns_only_allowed_sources",
    "scope_violations_are_loud_not_fallback",
}


def tokens(raw):
    return [match for match in TOKEN.finditer(raw)
            if not match.group().isspace()
            and not match.group().startswith(("//", "/*"))]


def assertions_in_body(raw, body, declaration):
    parts = tokens(body)
    found = []
    for i, token in enumerate(parts[:-2]):
        if token.group() not in {"assert", "assert_eq", "assert_ne"} or parts[i + 1].group() != "!":
            continue
        stack = []
        end = None
        for part in parts[i + 2:]:
            value = part.group()
            if value in {"(", "[", "{"}:
                stack.append(value)
            elif value in {")", "]", "}"}:
                expected = {")": "(", "]": "[", "}": "{"}[value]
                if not stack or stack.pop() != expected:
                    raise ValueError("Unbalanced assertion")
                if not stack:
                    end = part.end()
                    break
        if end is None:
            raise ValueError("Assertion has no closing delimiter")
        found.append({
            "line": raw[:declaration.start() + token.start()].count("\n") + 1,
            "quote": body[token.start():end],
        })
    return found


def test_location(suite, test):
    source = ROOT / "rust/crates/lingxi-service/tests" / (suite + ".rs")
    raw = source.read_text()
    declaration = re.search(r"\bfn\s+" + re.escape(test) + r"\s*\(", raw)
    if not declaration:
        raise ValueError("Missing actual test: " + test)
    level = 0
    opened = False
    for token in tokens(raw[declaration.end():]):
        if token.group() == "{":
            opened = True
            level += 1
        elif token.group() == "}":
            level -= 1
            if opened and level == 0:
                end = declaration.end() + token.end()
                break
    else:
        raise ValueError("Cannot delimit actual test: " + test)
    return {
        "path": source.relative_to(ROOT).as_posix(), "test": test,
        "sourceSHA256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "testStartLine": raw[:declaration.start()].count("\n") + 1,
        "testEndLine": raw[:end].count("\n") + 1,
        "assertions": assertions_in_body(raw, raw[declaration.start():end], declaration),
    }


def inspect(apply=False):
    inventory = json.loads(MAP.read_text())
    checked = []
    missing = []
    updates = []
    for row in inventory["items"]:
        for group in row["groups"]:
            for anchor in group.get("proofLocations", []):
                source = ROOT / anchor["path"]
                raw = source.read_text()
                digest = hashlib.sha256(source.read_bytes()).hexdigest()
                if anchor.get("sourceSHA256") == digest:
                    continue
                name = anchor["test"].rsplit("::", 1)[-1]
                declaration = re.search(r"\bfn\s+" + re.escape(name) + r"\s*\(", raw)
                if declaration is None:
                    missing.append({"id": row["id"], "test": name, "reason": "test missing"})
                    continue
                function_tokens = tokens(raw[declaration.end():])
                level = 0
                opened = False
                end = None
                for token in function_tokens:
                    if token.group() == "{":
                        level += 1
                        opened = True
                    elif token.group() == "}":
                        level -= 1
                        if opened and level == 0:
                            end = declaration.end() + token.end()
                            break
                if end is None:
                    raise ValueError(f"Cannot delimit test body: {source}:{name}")
                body = raw[declaration.start():end]
                body_tokens = tokens(body)
                token_values = [token.group() for token in body_tokens]
                locations = []
                for assertion in anchor.get("assertions", []):
                    needle = [token.group() for token in tokens(assertion["quote"])]
                    starts = [i for i in range(len(token_values) - len(needle) + 1)
                              if token_values[i:i + len(needle)] == needle]
                    if not starts:
                        missing.append({
                            "id": row["id"], "test": name,
                            "reason": "recorded assertion no longer exists in current test",
                            "quote": assertion["quote"],
                        })
                        continue
                    at = min(starts, key=lambda i: abs(
                        raw[:declaration.start() + body_tokens[i].start()].count("\n") + 1
                        - assertion["line"]))
                    start = body_tokens[at].start()
                    stop = body_tokens[at + len(needle) - 1].end()
                    line = raw[:declaration.start() + start].count("\n") + 1
                    locations.append({"line": line, "quote": body[start:stop]})
                checked.append({"id": row["id"], "test": name, "source": anchor["path"],
                                "assertionsChecked": len(locations)})
                if name in REVIEWED_CHANGED_TESTS:
                    locations = assertions_in_body(raw, body, declaration)
                if len(locations) == len(anchor.get("assertions", [])) and locations:
                    updates.append((anchor, {
                        "sourceSHA256": digest,
                        "testStartLine": raw[:declaration.start()].count("\n") + 1,
                        "testEndLine": raw[:end].count("\n") + 1,
                        "assertions": locations,
                    }))
                elif name in REVIEWED_CHANGED_TESTS and locations:
                    updates.append((anchor, {
                        "sourceSHA256": digest,
                        "testStartLine": raw[:declaration.start()].count("\n") + 1,
                        "testEndLine": raw[:end].count("\n") + 1,
                        "assertions": locations,
                    }))
    REPORT.write_text(json.dumps({
        "scope": "Assertion location review only; no coverage or verdict promotion.",
        "checked": checked, "changedAssertionsReviewedInTests": sorted(REVIEWED_CHANGED_TESTS),
        "missing": missing, "eligibleAnchorUpdates": len(updates), "applied": apply,
    }, ensure_ascii=False, indent=2) + "\n")
    if apply:
        if len(updates) != len(checked):
            raise ValueError("Unreviewed assertion drift remains; refusing partial map update")
        for anchor, update in updates:
            anchor.update(update)
        MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"staleAnchors": len(checked), "missingAssertions": len(missing),
                      "eligibleAnchorUpdates": len(updates)}))
    return inventory, updates, missing


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--report", type=Path, default=REPORT)
    arguments = parser.parse_args()
    REPORT = arguments.report
    inspect(arguments.apply)
