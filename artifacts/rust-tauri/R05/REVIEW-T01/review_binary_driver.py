#!/usr/bin/env python3
"""REVIEW-T01 independent binary-chain driver (C02/C03/C04/C06).

Written by the reviewer from scratch. Drives the REAL lingxi-service binary
as a subprocess with a real argv + real --config file + temp home; the
provider far end is THIS script's own stub (a threaded http.server that
records every request). Nothing calls bootstrap_with_deps; nothing reuses
the implementer's test harness.

C02 twist (per the independent-review charter item 2): the runtime nonce
file is created by the STUB *after* the first provider request arrives —
the second request's tool message must carry content that did not exist
when the run was submitted, proving a real mid-run tool execution whose
real result rode the wire.
"""
import json
import os
import re
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BINARY = "rust/target/debug/lingxi-service"
OUT = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(OUT, exist_ok=True)

FAILURES = []


def check(name, cond, detail=""):
    tag = "PASS" if cond else "FAIL"
    print(f"  [{tag}] {name}" + (f" — {detail}" if detail and not cond else ""))
    if not cond:
        FAILURES.append((name, detail))


class Stub:
    """A scripted openai-completions endpoint that records everything."""

    def __init__(self, script, on_request=None):
        self.script = list(script)
        self.on_request = on_request
        self.requests = []
        self.lock = threading.Lock()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                raw = self.rfile.read(length)
                body = json.loads(raw.decode("utf-8")) if raw else None
                with outer.lock:
                    idx = len(outer.requests)
                    outer.requests.append({
                        "path": self.path,
                        "authorization": self.headers.get("Authorization"),
                        "body": body,
                    })
                if outer.on_request:
                    outer.on_request(idx, body)
                with outer.lock:
                    payload = outer.script.pop(0) if outer.script else None
                if payload is None:
                    data = json.dumps({"error": {"message": "stub script exhausted"}}).encode()
                    self.send_response(500)
                else:
                    data = payload.encode()
                    self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def endpoint(self):
        return f"http://127.0.0.1:{self.port}/v1"

    def hits(self):
        with self.lock:
            return len(self.requests)

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def tool_call_response(call_id, tool, arguments, prompt_toks, completion_toks):
    return json.dumps({
        "id": "chatcmpl-review",
        "model": "review-stub-lies",
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": None,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {"name": tool, "arguments": json.dumps(arguments)},
                }],
            },
        }],
        "usage": {"prompt_tokens": prompt_toks, "completion_tokens": completion_toks},
    })


def final_response(text, prompt_toks, completion_toks):
    return json.dumps({
        "id": "chatcmpl-review",
        "model": "review-stub-lies",
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {"role": "assistant", "content": text},
        }],
        "usage": {"prompt_tokens": prompt_toks, "completion_tokens": completion_toks},
    })


class ServiceProc:
    def __init__(self, args):
        self.argv = [os.path.abspath(BINARY)] + args
        self.proc = subprocess.Popen(
            self.argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addr = None
        self.ready_line = None
        deadline = time.time() + 30
        lines = []
        while time.time() < deadline:
            line = self.proc.stdout.readline()
            if not line:
                break
            lines.append(line.rstrip())
            if line.startswith("LINGXI_SERVICE_READY "):
                self.ready_line = line.strip()
                m = re.search(r"addr=(\S+)", line)
                self.addr = m.group(1)
                break
        if not self.addr:
            err = self.proc.stderr.read()
            raise RuntimeError(f"service never READY\nstdout:\n" + "\n".join(lines) + f"\nstderr:\n{err}")
        # drain stderr in the background so the child never blocks on a full pipe
        self._stderr_lines = []
        def drain():
            for l in self.proc.stderr:
                self._stderr_lines.append(l.rstrip())
        threading.Thread(target=drain, daemon=True).start()

    def stop(self):
        self.proc.send_signal(signal.SIGTERM)
        try:
            rc = self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            raise RuntimeError("service did not exit within 30s of SIGTERM")
        return rc


def http_json(addr, method, path, bearer, body=None):
    import http.client
    conn = http.client.HTTPConnection(addr, timeout=120)
    headers = {"Authorization": f"Bearer {bearer}", "Content-Type": "application/json"}
    conn.request(method, path, body=json.dumps(body) if body is not None else None,
                 headers=headers)
    resp = conn.getresponse()
    raw = resp.read()
    conn.close()
    return resp.status, (json.loads(raw.decode()) if raw else None)


def read_token(home):
    with open(os.path.join(home, "lingxi-service", "local-token.json")) as f:
        return json.load(f)["token"]


def runs_db(home):
    path = os.path.join(home, "lingxi-service", "data", "runs.db")
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    return db


def q1(db, sql, params=()):
    row = db.execute(sql, params).fetchone()
    return None if row is None else row[0]


def scenario_c02():
    print("== C02: real binary, real HTTP out, real tool gateway, real result back ==")
    home = tempfile.mkdtemp(prefix="review-c02-home-")
    workspace = tempfile.mkdtemp(prefix="review-c02-ws-")
    cfgdir = tempfile.mkdtemp(prefix="review-c02-cfg-")
    nonce = f"NONCE-{uuid.uuid4().hex}"
    marker = f"REVIEW-FINAL-{uuid.uuid4().hex[:12]}"

    # The stub creates the nonce file ONLY when the first request arrives —
    # the file did not exist at submission time.
    def on_request(idx, body):
        if idx == 0:
            with open(os.path.join(workspace, "nonce_probe.txt"), "w") as f:
                f.write(f"review runtime nonce: {nonce}\n")

    stub = Stub([
        tool_call_response("call_rev_1", "read", {"path": "nonce_probe.txt"}, 31, 13),
        final_response(f"answer carries {marker}", 41, 17),
    ], on_request=on_request)

    config = {
        "home": home,
        "workspace": workspace,
        "providers": {"main": {
            "protocol": "openai-completions",
            "endpoint": stub.endpoint(),
            "auth": {"kind": "apiKey", "apiKey": "sk-review-t01-A"},
        }},
        "models": {"chat": {"provider": "main", "model": "stub-model-review"}},
    }
    cfgpath = os.path.join(cfgdir, "service.json")
    with open(cfgpath, "w") as f:
        json.dump(config, f)

    child = ServiceProc(["--home", home, "--config", cfgpath, "--bind", "127.0.0.1:0"])
    check("ready line names cli home source", "source=cli" in child.ready_line, child.ready_line)
    token = read_token(home)

    st, _ = http_json(child.addr, "POST", "/lingxi/v1/sessions/sess_local_alpha/execute",
                      "wrong-token", {"input": "read the nonce probe"})
    check("wrong token refused with 401", st == 401, f"got {st}")

    st, accepted = http_json(child.addr, "POST", "/lingxi/v1/sessions/sess_local_alpha/execute",
                             token, {"input": "read the nonce probe"})
    check("authenticated submit accepted", st == 200, f"got {st}: {accepted}")
    run_id = accepted.get("runId") if accepted else None
    check("acceptance carries runId", bool(run_id), str(accepted))

    check("exactly two provider turns hit the wire", stub.hits() == 2, f"hits={stub.hits()}")
    reqs = stub.requests
    if len(reqs) == 2:
        for r in reqs:
            check("path is /v1/chat/completions", r["path"] == "/v1/chat/completions", r["path"])
            check("bearer credential is the configured key",
                  r["authorization"] == "Bearer sk-review-t01-A", str(r["authorization"]))
            check("request model is the configured route model",
                  r["body"].get("model") == "stub-model-review", str(r["body"].get("model")))
        tools = reqs[0]["body"].get("tools") or []
        check("turn 1 declares the read tool",
              any(t.get("function", {}).get("name") == "read" for t in tools),
              json.dumps(tools)[:200])
        msgs1 = reqs[0]["body"].get("messages") or []
        check("turn 1 carries only the user submission", len(msgs1) == 1, str(len(msgs1)))
        msgs2 = reqs[1]["body"].get("messages") or []
        check("turn 2 = user + assistant(tool_calls) + tool result", len(msgs2) == 3,
              str(len(msgs2)))
        if len(msgs2) == 3:
            check("assistant tool call id pairs", msgs2[1]["tool_calls"][0]["id"] == "call_rev_1")
            check("tool message pairs the provider call id",
                  msgs2[2].get("role") == "tool" and msgs2[2].get("tool_call_id") == "call_rev_1",
                  json.dumps(msgs2[2])[:200])
            content = msgs2[2].get("content", "")
            check("tool message carries the RUNTIME-CREATED nonce (real tool result)",
                  nonce in content, content[:200])

    db = runs_db(home)
    check("run settled completed", q1(db, "SELECT status FROM runs WHERE run_id=?", (run_id,)) == "completed")
    check("terminal reason completed.with_final",
          q1(db, "SELECT terminal_reason FROM runs WHERE run_id=?", (run_id,)) == "completed.with_final")
    check("two model_call_completed events",
          q1(db, "SELECT COUNT(*) FROM key_events WHERE run_id=? AND event_type='model_call_completed'", (run_id,)) == 2)
    check("one tool_call_completed event",
          q1(db, "SELECT COUNT(*) FROM key_events WHERE run_id=? AND event_type='tool_call_completed'", (run_id,)) == 1)
    payload = q1(db, "SELECT payload_json FROM key_events WHERE run_id=? AND event_type='model_call_completed' ORDER BY event_id LIMIT 1", (run_id,)) or ""
    check("provider-reported usage persisted", '"inputTokens":"31"' in payload and '"outputTokens":"13"' in payload, payload[:200])
    started = q1(db, "SELECT payload_json FROM key_events WHERE run_id=? AND event_type='model_call_started' ORDER BY event_id LIMIT 1", (run_id,)) or ""
    check("persisted identity is the route (provider main)",
          '"provider":"main"' in started or '"provider": "main"' in started, started[:200])
    check("stub's claimed model never persisted", "review-stub-lies" not in started, started[:200])
    final_msg = q1(db, "SELECT content_json FROM messages WHERE run_id=?", (run_id,)) or ""
    check("committed final carries the stub marker", marker in final_msg, final_msg[:200])
    db.close()

    rc = child.stop()
    check("graceful SIGTERM shutdown exits 0", rc == 0, f"rc={rc}")
    # raw wire evidence for the review record
    with open(os.path.join(OUT, "c02-stub-wire-capture.json"), "w") as f:
        json.dump({"nonce": nonce, "marker": marker,
                   "requests": [{"path": r["path"], "authorization": r["authorization"],
                                 "body": r["body"]} for r in stub.requests]}, f,
                  ensure_ascii=False, indent=1)
    stub.stop()


def scenario_c03():
    print("== C03: no model plane => honest unconfigured outcome ==")
    home = tempfile.mkdtemp(prefix="review-c03-home-")
    child = ServiceProc(["--home", home, "--bind", "127.0.0.1:0"])
    token = read_token(home)
    st, accepted = http_json(child.addr, "POST", "/lingxi/v1/sessions/sess_local_alpha/execute",
                             token, {"input": "this needs a model"})
    check("submit accepted", st == 200, f"got {st}: {accepted}")
    run_id = accepted.get("runId")
    db = runs_db(home)
    check("status completed",
          q1(db, "SELECT status FROM runs WHERE run_id=?", (run_id,)) == "completed")
    check("terminal reason is the explicit unconfigured outcome",
          q1(db, "SELECT terminal_reason FROM runs WHERE run_id=?", (run_id,))
          == "completed.no_final.no_provider_configured",
          str(q1(db, "SELECT terminal_reason FROM runs WHERE run_id=?", (run_id,))))
    check("zero model calls started",
          q1(db, "SELECT COUNT(*) FROM key_events WHERE run_id=? AND event_type='model_call_started'", (run_id,)) == 0)
    check("zero tool calls started",
          q1(db, "SELECT COUNT(*) FROM key_events WHERE run_id=? AND event_type='tool_call_started'", (run_id,)) == 0)
    check("no fabricated message row",
          q1(db, "SELECT COUNT(*) FROM messages WHERE run_id=?", (run_id,)) == 0)
    db.close()
    rc = child.stop()
    check("clean shutdown", rc == 0, f"rc={rc}")


def scenario_c06():
    print("== C06: configured-but-unimplemented family => refusal with ZERO requests ==")
    home = tempfile.mkdtemp(prefix="review-c06-home-")
    workspace = tempfile.mkdtemp(prefix="review-c06-ws-")
    cfgdir = tempfile.mkdtemp(prefix="review-c06-cfg-")
    stub = Stub([])  # any contact would be a loud 500 — there must be none
    config = {
        "home": home,
        "workspace": workspace,
        "providers": {"main": {
            "protocol": "anthropic-messages",
            "endpoint": stub.endpoint(),
            "auth": {"kind": "none"},
        }},
        "models": {"chat": {"provider": "main", "model": "claude-review"}},
    }
    cfgpath = os.path.join(cfgdir, "service.json")
    with open(cfgpath, "w") as f:
        json.dump(config, f)
    child = ServiceProc(["--home", home, "--config", cfgpath, "--bind", "127.0.0.1:0"])
    token = read_token(home)
    st, accepted = http_json(child.addr, "POST", "/lingxi/v1/sessions/sess_local_alpha/execute",
                             token, {"input": "hello"})
    check("submit accepted", st == 200, f"got {st}: {accepted}")
    run_id = accepted.get("runId")
    db = runs_db(home)
    check("run failed loudly", q1(db, "SELECT status FROM runs WHERE run_id=?", (run_id,)) == "failed",
          str(q1(db, "SELECT status FROM runs WHERE run_id=?", (run_id,))))
    check("reason failed.provider_error",
          q1(db, "SELECT terminal_reason FROM runs WHERE run_id=?", (run_id,)) == "failed.provider_error",
          str(q1(db, "SELECT terminal_reason FROM runs WHERE run_id=?", (run_id,))))
    check("external request counter is ZERO", stub.hits() == 0, f"hits={stub.hits()}")
    db.close()
    rc = child.stop()
    check("clean shutdown", rc == 0, f"rc={rc}")
    stub.stop()


def scenario_c04():
    print("== C04: same model id, two providers, DISTINCT keys — attribution never crosses ==")
    home = tempfile.mkdtemp(prefix="review-c04-home-")
    workspace = tempfile.mkdtemp(prefix="review-c04-ws-")
    cfgdir = tempfile.mkdtemp(prefix="review-c04-cfg-")
    stub_a = Stub([final_response("from A (must never be served)", 5, 5)])
    stub_b = Stub([
        final_response("from B run 1", 7, 3),
        final_response("from B run 2", 8, 4),
    ])
    config = {
        "home": home,
        "workspace": workspace,
        "providers": {
            "alpha": {
                "protocol": "openai-completions",
                "endpoint": stub_a.endpoint(),
                "auth": {"kind": "apiKey", "apiKey": "sk-review-AAAA"},
            },
            "beta": {
                "protocol": "openai-completions",
                "endpoint": stub_b.endpoint(),
                "auth": {"kind": "apiKey", "apiKey": "sk-review-BBBB"},
            },
        },
        # BOTH providers would accept this model id; the route pins the pair.
        "models": {"chat": {"provider": "beta", "model": "shared-review-model"}},
    }
    cfgpath = os.path.join(cfgdir, "service.json")
    with open(cfgpath, "w") as f:
        json.dump(config, f)
    child = ServiceProc(["--home", home, "--config", cfgpath, "--bind", "127.0.0.1:0"])
    token = read_token(home)
    run_ids = []
    for sess in ("sess_local_alpha", "sess_local_beta"):
        st, accepted = http_json(child.addr, "POST", f"/lingxi/v1/sessions/{sess}/execute",
                                 token, {"input": f"hi from {sess}"})
        check(f"submit {sess} accepted", st == 200, f"got {st}")
        run_ids.append((sess, accepted.get("runId")))
    check("provider A endpoint received ZERO requests", stub_a.hits() == 0,
          f"hits={stub_a.hits()}")
    check("provider B endpoint served both runs", stub_b.hits() == 2, f"hits={stub_b.hits()}")
    for r in stub_b.requests:
        check("B's requests carry ONLY B's key", r["authorization"] == "Bearer sk-review-BBBB",
              str(r["authorization"]))
        check("B's requests name the shared model id",
              r["body"].get("model") == "shared-review-model", str(r["body"].get("model")))
    leaked = [r for r in stub_a.requests if r["authorization"] and "AAAA" in r["authorization"]]
    check("A's key never rode any wire", not leaked and stub_a.hits() == 0)
    db = runs_db(home)
    for sess, run_id in run_ids:
        payload = q1(db, "SELECT payload_json FROM key_events WHERE run_id=? AND event_type='model_call_started' ORDER BY event_id LIMIT 1", (run_id,)) or ""
        check(f"persisted identity of {sess} is (beta, shared-review-model)",
              '"provider":"beta"' in payload and '"model":"shared-review-model"' in payload,
              payload[:200])
    db.close()
    rc = child.stop()
    check("clean shutdown", rc == 0, f"rc={rc}")
    stub_a.stop()
    stub_b.stop()


def main():
    scenario_c02()
    scenario_c03()
    scenario_c06()
    scenario_c04()
    print()
    if FAILURES:
        print(f"REVIEW DRIVER RESULT: FAIL ({len(FAILURES)} failed checks)")
        for name, detail in FAILURES:
            print(f"  - {name}: {detail}")
        sys.exit(1)
    print("REVIEW DRIVER RESULT: ALL CHECKS PASS")


if __name__ == "__main__":
    main()
