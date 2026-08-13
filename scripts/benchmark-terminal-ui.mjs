import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { monitorEventLoopDelay, performance } from 'node:perf_hooks';
import { TerminalSessionManager } from '../lib/terminal/terminal-session-manager.ts';
import { createTerminalWsBridge } from '../server/terminal-ws-bridge.ts';
import { createTerminalOutputStream } from '../desktop/src/react/services/terminal-output-stream.ts';

function fakeBackend() {
  const handles = [];
  return {
    handles,
    spawn(options) {
      const handle = {
        emit(data) { options.onData(data); },
        exit(exitCode = 0) { options.onExit({ exitCode, signal: null }); },
        write() {},
        kill() { options.onExit({ exitCode: null, signal: 'SIGTERM' }); },
      };
      handles.push(handle);
      return handle;
    },
  };
}

function directoryBytes(root) {
  let total = 0;
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const filePath = path.join(root, entry.name);
    total += entry.isDirectory() ? directoryBytes(filePath) : fs.statSync(filePath).size;
  }
  return total;
}

function resourceDelta(before, after) {
  return {
    userCpuMs: (after.userCPUTime - before.userCPUTime) / 1_000,
    systemCpuMs: (after.systemCPUTime - before.systemCPUTime) / 1_000,
    fsReadOps: after.fsRead - before.fsRead,
    fsWriteOps: after.fsWrite - before.fsWrite,
  };
}

function delayMs(value) {
  return Number.isFinite(value) ? Number((value / 1e6).toFixed(3)) : null;
}

async function runScenario({ name, terminalCount, produce }) {
  const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-terminal-benchmark-'));
  const backend = fakeBackend();
  const expandedStream = createTerminalOutputStream();
  const collapsedStream = createTerminalOutputStream();
  const bridgeRef = { current: null };
  let outputBatches = 0;
  let outputBytes = 0;
  let rendererDeliveriesExpanded = 0;
  const rendererDeliveriesCollapsed = 0;
  let firstStateAt = null;
  const manager = new TerminalSessionManager({
    lingxiHome,
    createBackend: () => backend,
    getSessionIdForPath: (sessionPath) => `sess_${path.basename(sessionPath, '.jsonl')}`,
    emitEvent: (event, sessionPath) => bridgeRef.current?.handleEvent(event, sessionPath),
  });
  bridgeRef.current = createTerminalWsBridge({
    terminalSessions: manager,
    resolveSessionId: (sessionPath) => `sess_${path.basename(sessionPath, '.jsonl')}`,
    broadcast: (message) => {
      if (message.type === 'terminal_state' && firstStateAt === null) firstStateAt = performance.now();
      if (message.type !== 'terminal_output') return;
      outputBatches += 1;
      outputBytes += message.chunks.reduce((sum, chunk) => sum + Buffer.byteLength(chunk.data, 'utf8'), 0);
      expandedStream.handleChunks(message);
      collapsedStream.handleChunks(message);
    },
  });

  const sessionPath = path.join(lingxiHome, 'agents', 'hana', 'sessions', `${name}.jsonl`);
  const startedAt = performance.now();
  const terminals = [];
  for (let index = 0; index < terminalCount; index += 1) {
    const terminal = await manager.start({
      sessionPath,
      agentId: 'hana',
      cwd: lingxiHome,
      command: `benchmark-${name}-${index}`,
      label: `${name}-${index}`,
    });
    terminals.push(terminal);
    const ref = {
      terminalId: terminal.terminalId,
      sessionId: terminal.sessionId,
      sessionPath,
    };
    expandedStream.subscribe(ref, {
      onChunks: () => { rendererDeliveriesExpanded += 1; },
    });
    expandedStream.handleTail({
      type: 'terminal_tail',
      ...ref,
      terminal,
      chunks: [],
      sinceSeq: null,
      lastSeq: 0,
      truncated: false,
    });
    // 折叠态刻意没有 subscriber；若实现偷偷缓存，交付计数会暴露异常。
  }
  const firstStateLatencyMs = firstStateAt === null ? null : firstStateAt - startedAt;

  const delay = monitorEventLoopDelay({ resolution: 10 });
  delay.enable();
  // 先让采样计时器进入事件循环，再清零统计；否则紧接着发生的同步突发会在首个
  // 采样点建立前结束，错误地把真正的阻塞时间漏掉。
  await new Promise((resolve) => setTimeout(resolve, 20));
  delay.reset();
  let eventLoopProbeMaxMs = 0;
  let expectedProbeAt = performance.now() + 10;
  const eventLoopProbe = setInterval(() => {
    const now = performance.now();
    eventLoopProbeMaxMs = Math.max(eventLoopProbeMaxMs, now - expectedProbeAt);
    expectedProbeAt = now + 10;
  }, 10);
  const resourcesBefore = process.resourceUsage();
  const benchStart = performance.now();
  await produce({ handles: backend.handles, terminals });
  backend.handles.forEach((handle) => handle.exit(0));
  await new Promise((resolve) => setTimeout(resolve, 60));
  const durationMs = performance.now() - benchStart;
  const resourcesAfter = process.resourceUsage();
  clearInterval(eventLoopProbe);
  delay.disable();

  const report = {
    name,
    terminalCount,
    durationMs: Number(durationMs.toFixed(3)),
    firstStateLatencyMs: firstStateLatencyMs === null ? null : Number(firstStateLatencyMs.toFixed(3)),
    outputBytes,
    outputBatches,
    outputBatchesPerSecond: Number((outputBatches / Math.max(durationMs / 1_000, 0.001)).toFixed(2)),
    rendererDeliveriesExpanded,
    rendererDeliveriesCollapsed,
    eventLoopDelayMs: {
      mean: delayMs(delay.mean),
      max: delayMs(delay.max),
      p99: delayMs(delay.percentile(99)),
      intervalProbeMax: Number(eventLoopProbeMaxMs.toFixed(3)),
    },
    diskBytes: directoryBytes(lingxiHome),
    ...resourceDelta(resourcesBefore, resourcesAfter),
  };
  fs.rmSync(lingxiHome, { recursive: true, force: true });
  return report;
}

const tenMb = 10 * 1024 * 1024;
const reports = [];

reports.push(await runScenario({
  name: 'continuous-10mb',
  terminalCount: 1,
  produce: async ({ handles }) => {
    const chunk = 'x'.repeat(4 * 1024);
    for (let written = 0; written < tenMb; written += Buffer.byteLength(chunk)) handles[0].emit(chunk);
  },
}));

reports.push(await runScenario({
  name: 'small-chunks-high-frequency',
  terminalCount: 1,
  produce: async ({ handles }) => {
    await new Promise((resolve) => {
      let emitted = 0;
      const timer = setInterval(() => {
        handles[0].emit('small-output\n');
        emitted += 1;
        if (emitted >= 200) {
          clearInterval(timer);
          resolve();
        }
      }, 5);
    });
  },
}));

reports.push(await runScenario({
  name: 'four-terminals-interleaved',
  terminalCount: 4,
  produce: async ({ handles }) => {
    const chunk = 'm'.repeat(4 * 1024);
    for (let round = 0; round < 256; round += 1) {
      for (const handle of handles) handle.emit(chunk);
    }
  },
}));

console.log(JSON.stringify({
  generatedAt: new Date().toISOString(),
  note: 'rendererDeliveriesExpanded is the subscriber delivery count and an upper bound for React state commits; collapsed has no subscriber.',
  reports,
}, null, 2));
