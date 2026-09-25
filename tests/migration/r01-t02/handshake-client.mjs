// R01-A04 handshake client — drives the deterministic local prototype
// server (lingxi-proto-server) over real HTTP and real WebSocket and
// records the full interaction transcript.
//
// Scenarios (5 connections, matching the server's --connections 5):
//   http-ok                 POST /lingxi/v1/handshake, client 1..=1 → 200 selected 1
//   http-too-new            POST, client 2..=2                     → 400 version_incompatible
//   http-too-old            POST, client 0..=0                     → 400 version_incompatible
//   ws-ok                   WS hello 1..=1 → ServerHello frame + close 1000
//   ws-too-new              WS hello 3..=3 → error frame + close 4409
//
// Assertions are hard: a scenario that gets the wrong status, a missing
// version_incompatible code, or a silently "selected" fallback version
// fails the run. The transcript (every request/response/frame, byte-level
// where relevant) is written as JSONL to --transcript.

import fs from "node:fs";

const args = process.argv.slice(2);
function opt(name) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : null;
}
const port = Number(opt("--port"));
const transcriptPath = opt("--transcript");
if (!port || !transcriptPath) {
  console.error("usage: handshake-client.mjs --port P --transcript FILE");
  process.exit(2);
}

const transcript = [];
const failures = [];

function record(entry) {
  transcript.push({ at: new Date().toISOString(), ...entry });
}

function fail(scenario, message) {
  failures.push(`${scenario}: ${message}`);
}

function hello(min, max, extra = {}) {
  return {
    protocol: "lingxi.wire",
    clientKind: "roundtrip-probe",
    clientVersion: "0.0.0-probe",
    protocolMin: min,
    protocolMax: max,
    ...extra,
  };
}

async function httpScenario(scenario, body, expect) {
  const raw = JSON.stringify(body);
  record({ scenario, direction: "c->s", transport: "http", body: raw });
  const res = await fetch(`http://127.0.0.1:${port}/lingxi/v1/handshake`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: raw,
  });
  const text = await res.text();
  record({
    scenario,
    direction: "s->c",
    transport: "http",
    status: res.status,
    body: text,
  });
  let parsed = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    fail(scenario, "response is not JSON");
    return;
  }
  if (res.status !== expect.status) {
    fail(scenario, `HTTP status ${res.status}, expected ${expect.status}`);
  }
  if (expect.selectedProtocol !== undefined) {
    if (parsed.selectedProtocol !== expect.selectedProtocol) {
      fail(scenario, `selectedProtocol=${parsed.selectedProtocol}, expected ${expect.selectedProtocol}`);
    }
  }
  if (expect.errorCode !== undefined) {
    if (parsed.code !== expect.errorCode) {
      fail(scenario, `error code=${parsed.code}, expected ${expect.errorCode}`);
    }
    // "no default guessing": a rejection must not carry any selected version
    if ("selectedProtocol" in parsed) {
      fail(scenario, "rejection carried selectedProtocol — server guessed a default");
    }
    const d = parsed.details ?? {};
    for (const key of ["supportedMin", "supportedMax", "clientMin", "clientMax"]) {
      if (!(key in d)) fail(scenario, `error details missing ${key} (not diagnosable)`);
    }
    if (parsed.retryable !== false) fail(scenario, "version_incompatible must be non-retryable");
  }
}

function wsScenario(scenario, helloBody, expect) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:${port}/lingxi/v1/ws`);
    const frames = [];
    ws.onopen = () => {
      const raw = JSON.stringify(helloBody);
      record({ scenario, direction: "c->s", transport: "ws", frame: "text", body: raw });
      ws.send(raw);
    };
    ws.onmessage = (event) => {
      frames.push(event.data);
      record({ scenario, direction: "s->c", transport: "ws", frame: "text", body: String(event.data) });
    };
    ws.onerror = (e) => {
      record({ scenario, direction: "s->c", transport: "ws", frame: "error", body: String(e) });
    };
    ws.onclose = (event) => {
      record({
        scenario,
        direction: "s->c",
        transport: "ws",
        frame: "close",
        code: event.code,
        reason: event.reason,
      });
      if (frames.length !== 1) {
        fail(scenario, `expected exactly 1 text frame, got ${frames.length}`);
        return resolve();
      }
      let parsed = null;
      try {
        parsed = JSON.parse(frames[0]);
      } catch {
        fail(scenario, "frame is not JSON");
        return resolve();
      }
      if (expect.selectedProtocol !== undefined) {
        if (parsed.selectedProtocol !== expect.selectedProtocol) {
          fail(scenario, `selectedProtocol=${parsed.selectedProtocol}, expected ${expect.selectedProtocol}`);
        }
        if (event.code !== 1000) fail(scenario, `close code ${event.code}, expected 1000`);
      }
      if (expect.errorCode !== undefined) {
        if (parsed.code !== expect.errorCode) {
          fail(scenario, `error code=${parsed.code}, expected ${expect.errorCode}`);
        }
        if ("selectedProtocol" in parsed) {
          fail(scenario, "rejection carried selectedProtocol — server guessed a default");
        }
        if (event.code !== expect.closeCode) {
          fail(scenario, `close code ${event.code}, expected ${expect.closeCode}`);
        }
      }
      resolve();
    };
  });
}

await httpScenario("http-ok", hello(1, 1, { caps: ["events.v1", "teleport"] }), {
  status: 200,
  selectedProtocol: 1,
});
await httpScenario("http-too-new", hello(2, 2), { status: 400, errorCode: "version_incompatible" });
await httpScenario("http-too-old", hello(0, 0), { status: 400, errorCode: "version_incompatible" });
await wsScenario("ws-ok", hello(1, 1), { selectedProtocol: 1 });
await wsScenario("ws-too-new", hello(3, 3), {
  errorCode: "version_incompatible",
  closeCode: 4409,
});

// Extra positive detail: unknown caps must be reported, not silently dropped.
const okEntry = transcript.find(
  (t) => t.scenario === "http-ok" && t.direction === "s->c",
);
if (okEntry) {
  const body = JSON.parse(okEntry.body);
  if (!Array.isArray(body.rejectedCaps) || !body.rejectedCaps.includes("teleport")) {
    fail("http-ok", "unknown cap 'teleport' was not reported in rejected_caps");
  }
}

fs.writeFileSync(
  transcriptPath,
  transcript.map((t) => JSON.stringify(t)).join("\n") + "\n",
);

if (failures.length > 0) {
  console.error(`HANDSHAKE FAILURES (${failures.length}):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(`OK: 5 handshake scenarios recorded to ${transcriptPath}`);
for (const t of transcript.filter((t) => t.direction === "s->c")) {
  const what =
    t.transport === "http"
      ? `HTTP ${t.status}`
      : t.frame === "close"
        ? `WS close ${t.code} ${t.reason ?? ""}`
        : `WS frame ${String(t.body).slice(0, 80)}`;
  console.log(`  ${t.scenario}: ${what}`);
}
