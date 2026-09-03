import fs from "node:fs";
import process from "node:process";
import readline from "node:readline";

const token = process.env.LINGXI_LOCAL_MODEL_TOKEN || "";
const protocol = Number(process.env.LINGXI_LOCAL_MODEL_PROTOCOL || 0);
const attemptFile = process.env.LINGXI_LOCAL_MODEL_ATTEMPT_FILE;
let attempt = 1;
if (attemptFile) {
  const previous = fs.existsSync(attemptFile) ? Number(fs.readFileSync(attemptFile, "utf8") || 0) : 0;
  attempt = previous + 1;
  fs.writeFileSync(attemptFile, String(attempt));
}

if (process.env.LINGXI_LOCAL_MODEL_FAIL_MODE === "always") {
  process.stdout.write(`${JSON.stringify({ type: "wrong" })}\n`);
} else if (process.env.LINGXI_LOCAL_MODEL_FAIL_MODE === "first" && attempt === 1) {
  process.stdout.write(`${JSON.stringify({ type: "wrong" })}\n`);
} else {
  process.stdout.write(`${JSON.stringify({
    type: "ready",
    protocol,
    token,
    runtimeId: "fixture-runtime",
    runtimeVersion: "1",
    backend: "cpu",
    pid: process.pid,
  })}\n`);
}

process.stderr.write(`fixture token=${token}\n`);

const input = readline.createInterface({ input: process.stdin });
input.on("line", (line) => {
  const message = JSON.parse(line);
  if (message.type === "shutdown") {
    if (process.env.LINGXI_LOCAL_MODEL_IGNORE_SHUTDOWN !== "1") process.exit(0);
    return;
  }
  if (message.type !== "request") return;
  if (message.method === "hang") return;
  if (message.method === "crash") process.exit(17);
  if (message.method === "inspect_env") {
    process.stdout.write(`${JSON.stringify({
      type: "response",
      id: message.id,
      ok: true,
      result: {
        leaked: process.env.SHOULD_NOT_LEAK || null,
        allowed: process.env.LINGXI_LOCAL_MODEL_FIXTURE || null,
      },
    })}\n`);
    return;
  }
  process.stdout.write(`${JSON.stringify({ type: "response", id: message.id, ok: true, result: message.payload })}\n`);
});
