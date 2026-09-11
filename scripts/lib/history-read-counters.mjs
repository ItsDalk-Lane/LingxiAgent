/**
 * 历史分页读取基准 —— fs / JSON.parse / JSON.stringify 计数器（任务 A05 仪表）。
 *
 * 设计约束（与 A04/A05 任务书一致）：
 *  - 不修改任何生产代码；通过运行时包装全局 `fs`、`fs.promises`、`JSON` 得到逻辑计数。
 *  - 覆盖 sync + async 入口：readFileSync / readFile / fs.promises.readFile /
 *    openSync+readSync 循环（SDK SessionManager.loadEntriesFromFile / readSessionHeader 的
 *    实际读取形态）/ fs.promises.open 返回的 FileHandle.read（looksLikePiSessionFile 形态）/
 *    createReadStream（仅计数，生产读取路径未触达时如实为零）。
 *  - fullFileReadCalls 口径（scripts/benchmark 报告中复述）：
 *      a) readFile/readFileSync 等一次性整文件读 API 对会话 JSONL 的调用；
 *      b) 一个 fd 上的顺序读 episode（open→…→close）若从偏移 0 起顺序覆盖 ≥ 打开时文件
 *         长度，也计一次整文件读（这正是 SDK loadEntriesFromFile 的循环全量装载）；
 *      c) 有界部分读（如 readSessionHeader 的 4KiB 头扫描、512B Pi 头探测）不计入。
 *  - 逻辑读字节是包装层读到的字节数，不代表物理磁盘 I/O；OS page cache 状态未知，如实注明。
 *  - AsyncLocalStorage 把每次调用归因到当前请求；请求外的调用归入 global（harness）桶。
 */

import { AsyncLocalStorage } from "node:async_hooks";
import fs from "node:fs";
import fsPromises from "node:fs/promises";
import path from "node:path";

/** @type {ReturnType<typeof createHistoryReadCounters> | null} */
let activeCounters = null;

export function getActiveHistoryReadCounters() {
  return activeCounters;
}

function isSessionShapedJsonLine(text) {
  return (
    typeof text === "string" &&
    text.length >= 10 &&
    text.charCodeAt(0) === 0x7b && // {
    (text.startsWith('{"type":') || text.startsWith('{"type": '))
  );
}

function isMessagesResponseObject(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    "messages" in value &&
    "nextBefore" in value &&
    "hasMore" in value
  );
}

export function createHistoryReadCounters({ label = "history-read" } = {}) {
  if (activeCounters) {
    throw new Error(
      `[${label}] 已有活动的 history-read 计数器实例（${activeCounters.label}）——全局 fs/JSON 包装只允许一份，拒绝静默叠加`,
    );
  }

  const als = new AsyncLocalStorage();

  // ── 原函数快照（install 一次，uninstall 恢复）──
  const originals = {
    fs: {},
    fsPromises: {},
    jsonParse: JSON.parse,
    jsonStringify: JSON.stringify,
  };

  const installed = {
    fsReadFileSync: false,
    fsReadFile: false,
    fsPromisesReadFile: false,
    fsOpenSync: false,
    fsOpen: false,
    fsPromisesOpen: false,
    fsReadSync: false,
    fsCloseSync: false,
    fsCreateReadStream: false,
    fsWriteFileSync: false,
    fsCopyFileSync: false,
    json: false,
  };

  /** fd → episode 记录；open 时捕获当时作用域，close 时归还归因 */
  const fdEpisodes = new Map();
  let nextFdKey = 1;

  const instance = {
    label,
    global: null,
    requestSeq: 0,

    reset() {
      this.zeroTemplate = this.makeZeroGauges();
      this.global = structuredClone(this.zeroTemplate);
    },

    makeZeroGauges() {
      return {
        readCalls: 0,
        fullFileReadCalls: 0,
        fullFileReadViaReadApi: 0,
        fullFileReadViaFdEpisode: 0,
        sessionFullFileReadCalls: 0,
        shortReadEvents: 0,
        eofReads: 0,
        logicalReadBytes: 0,
        sessionFileReadBytes: 0,
        readMs: 0,
        jsonlParseCount: 0,
        jsonlParseMs: 0,
        parseMs: 0,
        serializeMs: 0,
        responseUtf8Bytes: 0,
        writeCalls: 0,
        writtenBytes: 0,
        writeMs: 0,
        sessionFileWriteCalls: 0,
        repairBackupCopyCalls: 0,
        statCalls: 0,
        branchOpenCalls: 0,
        getBranchCalls: 0,
        openThrows: 0,
        getBranchThrows: 0,
        scanCalls: {}, // name -> { calls, entriesVisited, ms }
      };
    },

    // ── 请求作用域 ──
    beginRequest(meta) {
      if (als.getStore()) {
        throw new Error(`[${label}] beginRequest 嵌套调用——上一请求未 endRequest，拒绝归因歧义`);
      }
      this.requestSeq += 1;
      const scope = {
        meta: { requestId: this.requestSeq, ...meta },
        gauges: this.newGauges(),
        memoryBefore: process.memoryUsage(),
        closed: false,
      };
      return {
        store: scope,
        run: (fn) => als.run(scope, fn),
      };
    },

    newGauges() {
      return structuredClone(this.zeroTemplate); // 结构一致、数值清零（不走 JSON.parse 包装）
    },

    endRequest(handle) {
      const scope = handle.store;
      if (!scope || scope.closed) throw new Error(`[${label}] endRequest：作用域不存在或已结束`);
      scope.closed = true;
      const memoryAfter = process.memoryUsage();
      return {
        meta: scope.meta,
        gauges: scope.gauges,
        memoryBefore: scope.memoryBefore,
        memoryAfter,
      };
    },

    currentScope() {
      return als.getStore() ?? null;
    },

    // ── 归因工具 ──
    g() {
      const scope = als.getStore();
      return scope ? scope.gauges : this.global;
    },

    recordBranchOpen(threw = false) {
      this.g().branchOpenCalls += 1;
      if (threw) this.g().openThrows += 1;
    },

    recordGetBranch(threw = false) {
      this.g().getBranchCalls += 1;
      if (threw) this.g().getBranchThrows += 1;
    },

    recordScan(name, inputLength, ms) {
      const target = als.getStore() ? als.getStore().gauges.scanCalls : this.global.scanCalls;
      const entry = target[name] || (target[name] = { calls: 0, entriesVisited: 0, ms: 0 });
      entry.calls += 1;
      if (Number.isFinite(inputLength)) entry.entriesVisited += inputLength;
      entry.ms += ms;
    },

    /** 汇总仪表快照（供请求记录 / 自测断言使用） */
    snapshotOf(gauges) {
      const scanCalls = gauges.scanCalls ?? {};
      let metadataVisitedCount = 0;
      let metadataScanCalls = 0;
      for (const entry of Object.values(scanCalls)) {
        metadataScanCalls += entry.calls;
        metadataVisitedCount += entry.entriesVisited;
      }
      return {
        ...gauges,
        scanCalls,
        // 口径：被包装的全数组扫描入口访问的条目总数（下界；路由内联扫描未计入）
        metadataVisitedCount,
        metadataScanCalls,
        // 口径：旧路径的完整历史投影以 getBranch 调用为 1:1 代理
        // （loadSessionHistoryMessages 分支路径 getBranch→projectBranchHistory；fallback 路径不调 getBranch）
        fullHistoryProjectionCount: gauges.getBranchCalls,
      };
    },
  };

  instance.reset();

  // ── 路径分类 ──
  const classify = (filePath, scope) => {
    if (!scope || !scope.meta.sessionPath) return "other";
    const resolved = path.resolve(String(filePath));
    const session = path.resolve(scope.meta.sessionPath);
    if (resolved === session) return "session-jsonl";
    if (resolved === `${session}.repair.json`) return "repair-backup";
    return "other";
  };

  const scopeOf = () => als.getStore();

  const accumulateRead = (scope, kind, bytes, ms) => {
    const g = scope ? scope.gauges : instance.global;
    g.readCalls += 1;
    if (process.env.HIST_DEBUG_READ_STACK && g.readCalls === 2000) {
      console.error("[HIST_DEBUG_READ_STACK] readCalls#2000 at:\n" + new Error().stack);
    }
    g.logicalReadBytes += bytes;
    g.readMs += ms;
    if (kind === "session-jsonl") g.sessionFileReadBytes += bytes;
  };

  // ── fs 包装 ──
  const wrapReadFileSync = (orig) =>
    function readFileSync(filePath, ...rest) {
      const t0 = process.hrtime.bigint();
      let result;
      try {
        result = orig.call(fs, filePath, ...rest);
      } finally {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        const scope = scopeOf();
        const kind = classify(filePath, scope);
        const bytes = Buffer.isBuffer(result) ? result.length : Buffer.byteLength(String(result ?? ""), "utf8");
        accumulateRead(scope, kind, bytes, ms);
        // readFile API 一次性读全文件
        if (kind === "session-jsonl") {
          const g = scope ? scope.gauges : instance.global;
          g.fullFileReadCalls += 1;
          g.fullFileReadViaReadApi += 1;
          g.sessionFullFileReadCalls += 1;
        }
      }
      return result;
    };

  const wrapReadFileCb = (orig) =>
    function readFile(filePath, ...rest) {
      const scope = scopeOf();
      const kind = classify(filePath, scope);
      const t0 = process.hrtime.bigint();
      const cbIdx = typeof rest[rest.length - 1] === "function" ? rest.length - 1 : -1;
      const callback = cbIdx >= 0 ? rest[cbIdx] : null;
      if (callback) {
        rest[cbIdx] = function patchedReadFileCb(err, data) {
          const ms = Number(process.hrtime.bigint() - t0) / 1e6;
          if (!err) {
            const bytes = Buffer.isBuffer(data)
              ? data.length
              : Buffer.byteLength(String(data ?? ""), "utf8");
            accumulateRead(scope, kind, bytes, ms);
            if (kind === "session-jsonl") {
              const g = scope ? scope.gauges : instance.global;
              g.fullFileReadCalls += 1;
              g.fullFileReadViaReadApi += 1;
              g.sessionFullFileReadCalls += 1;
            }
          }
          return callback(err, data);
        };
      }
      return orig.call(fs, filePath, ...rest);
    };

  // promises.readFile：需要拿到结果字节统计
  const wrapPromisesReadFileImpl = (orig) =>
    async function readFile(filePath, ...rest) {
      const scope = scopeOf();
      const kind = classify(filePath, scope);
      const t0 = process.hrtime.bigint();
      try {
        const data = await orig.call(fsPromises, filePath, ...rest);
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        const bytes = Buffer.isBuffer(data) ? data.length : Buffer.byteLength(String(data ?? ""), "utf8");
        accumulateRead(scope, kind, bytes, ms);
        if (kind === "session-jsonl") {
          const g = scope ? scope.gauges : instance.global;
          g.fullFileReadCalls += 1;
          g.fullFileReadViaReadApi += 1;
          g.sessionFullFileReadCalls += 1;
        }
        return data;
      } catch (error) {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        accumulateRead(scope, kind, 0, ms);
        throw error;
      }
    };

  /** fd episode：openSync 时登记，readSync 累计，closeSync 结算 */
  const registerEpisode = (fd, filePath, scope, sizeAtOpen) => {
    const key = fd;
    fdEpisodes.set(key, {
      fdKey: nextFdKey++,
      filePath: String(filePath),
      kind: classify(filePath, scope),
      scope,
      sizeAtOpen,
      coveredBytes: 0,
      sequential: true,
      nextExpectedOffset: 0,
      readCalls: 0,
      shortReadEvents: 0,
      eofReads: 0,
    });
  };

  const finalizeEpisode = (fd) => {
    const ep = fdEpisodes.get(fd);
    if (!ep) return;
    fdEpisodes.delete(fd);
    // readCalls/readMs/logicalReadBytes 已在 wrapReadSync / FileHandle.read 按调用累计；
    // 这里只做 episode 级别的整文件读判定。
    const g = ep.scope ? ep.scope.gauges : instance.global;
    if (ep.kind === "session-jsonl") {
      if (ep.sequential && ep.coveredBytes >= ep.sizeAtOpen && ep.sizeAtOpen >= 0 && ep.readCalls > 0) {
        g.fullFileReadCalls += 1;
        g.fullFileReadViaFdEpisode += 1;
        g.sessionFullFileReadCalls += 1;
      }
    }
  };

  const wrapOpenSync = (orig) =>
    function openSync(filePath, flags, mode) {
      const fd = orig.call(fs, filePath, flags, mode);
      const scope = scopeOf();
      let sizeAtOpen = -1;
      try {
        sizeAtOpen = fs.fstatSync(fd).size;
      } catch {}
      registerEpisode(fd, filePath, scope, sizeAtOpen);
      return fd;
    };

  const wrapOpenCb = (orig) =>
    function open(filePath, ...rest) {
      const scope = scopeOf();
      const cbIdx = typeof rest[rest.length - 1] === "function" ? rest.length - 1 : -1;
      const callback = cbIdx >= 0 ? rest[cbIdx] : null;
      if (!callback) return orig.call(fs, filePath, ...rest);
      rest[cbIdx] = function patchedOpenCb(err, fd) {
        if (!err) {
          let sizeAtOpen = -1;
          try {
            sizeAtOpen = fs.fstatSync(fd).size;
          } catch {}
          registerEpisode(fd, filePath, scope, sizeAtOpen);
        }
        return callback(err, fd);
      };
      return orig.call(fs, filePath, ...rest);
    };

  const wrapPromisesOpen = (orig) =>
    async function open(filePath, ...rest) {
      const handle = await orig.call(fsPromises, filePath, ...rest);
      const scope = scopeOf();
      let sizeAtOpen = -1;
      try {
        sizeAtOpen = (await handle.stat()).size;
      } catch {}
      registerEpisode(handle.fd, filePath, scope, sizeAtOpen);
      // FileHandle.read 走内部绑定，不经 fs.read 包装——按实例包装以覆盖
      // looksLikePiSessionFile（512B 头探测）与 readSessionTailUtf8（尾读）形态。
      const origRead = handle.read.bind(handle);
      handle.read = async (buffer, offset, length, position) => {
        const ep = fdEpisodes.get(handle.fd);
        const t0 = process.hrtime.bigint();
        try {
          const result = await origRead(buffer, offset, length, position);
          const ms = Number(process.hrtime.bigint() - t0) / 1e6;
          if (ep) {
            ep.readCalls += 1;
            ep.coveredBytes += result.bytesRead;
            if (typeof position === "number" && position !== ep.nextExpectedOffset) ep.sequential = false;
            ep.nextExpectedOffset =
              typeof position === "number" ? position + result.bytesRead : ep.nextExpectedOffset + result.bytesRead;
            const eg = ep.scope ? ep.scope.gauges : instance.global;
            if (result.bytesRead === 0) {
              ep.eofReads += 1;
              eg.eofReads += 1;
            } else if (result.bytesRead < length) {
              ep.shortReadEvents += 1;
              eg.shortReadEvents += 1;
            }
          }
          accumulateRead(ep ? ep.scope : scope, ep ? ep.kind : classify(filePath, scope), result.bytesRead, ms);
          return result;
        } catch (error) {
          const ms = Number(process.hrtime.bigint() - t0) / 1e6;
          accumulateRead(ep ? ep.scope : scope, ep ? ep.kind : "other", 0, ms);
          throw error;
        }
      };
      const origClose = handle.close.bind(handle);
      handle.close = async () => {
        finalizeEpisode(handle.fd);
        return origClose();
      };
      return handle;
    };

  const wrapReadSync = (orig) =>
    function readSync(fd, buffer, offset, length, position) {
      const t0 = process.hrtime.bigint();
      let bytesRead;
      try {
        bytesRead = orig.call(fs, fd, buffer, offset, length, position);
      } finally {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        const ep = fdEpisodes.get(fd);
        const g = ep && ep.scope ? ep.scope.gauges : instance.global;
        g.readCalls += 1;
        g.readMs += ms;
        g.logicalReadBytes += bytesRead ?? 0;
        if (ep && ep.kind === "session-jsonl") g.sessionFileReadBytes += bytesRead ?? 0;
        if (ep) {
          ep.readCalls += 1;
          if (typeof bytesRead === "number") {
            ep.coveredBytes += bytesRead;
            if (typeof position === "number" && position !== ep.nextExpectedOffset) ep.sequential = false;
            if (position === null || position === undefined) {
              ep.nextExpectedOffset += bytesRead;
            } else {
              ep.nextExpectedOffset = position + bytesRead;
            }
            if (bytesRead === 0) {
              ep.eofReads += 1;
              g.eofReads += 1;
            } else if (bytesRead < length) {
              ep.shortReadEvents += 1;
              g.shortReadEvents += 1;
            }
          }
        }
      }
      return bytesRead;
    };

  const wrapCloseSync = (orig) =>
    function closeSync(fd) {
      finalizeEpisode(fd);
      return orig.call(fs, fd);
    };

  const wrapCreateReadStream = (orig) =>
    function createReadStream(filePath, options) {
      const scope = scopeOf();
      const kind = classify(filePath, scope);
      const wholeFile = !options || (options.start === undefined && options.end === undefined);
      const stream = orig.call(fs, filePath, options);
      if (wholeFile) {
        const g = scope ? scope.gauges : instance.global;
        g.fullFileReadCalls += 1;
        g.fullFileReadViaReadApi += 1;
      }
      let bytes = 0;
      stream.on("data", (chunk) => {
        bytes += chunk.length;
      });
      stream.on("close", () => {
        accumulateRead(scope, kind, bytes, 0);
      });
      return stream;
    };

  const wrapWriteFileSync = (orig) =>
    function writeFileSync(filePath, data, ...rest) {
      const scope = scopeOf();
      const kind = classify(filePath, scope);
      const t0 = process.hrtime.bigint();
      try {
        return orig.call(fs, filePath, data, ...rest);
      } finally {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        const g = scope ? scope.gauges : instance.global;
        g.writeCalls += 1;
        g.writeMs += ms;
        g.writtenBytes += Buffer.isBuffer(data) ? data.length : Buffer.byteLength(String(data ?? ""), "utf8");
        if (kind === "session-jsonl") g.sessionFileWriteCalls += 1;
      }
    };

  const wrapCopyFileSync = (orig) =>
    function copyFileSync(src, dest, ...rest) {
      const scope = scopeOf();
      const kindDest = classify(dest, scope);
      try {
        return orig.call(fs, src, dest, ...rest);
      } finally {
        const g = scope ? scope.gauges : instance.global;
        if (kindDest === "repair-backup") g.repairBackupCopyCalls += 1;
      }
    };

  // ── JSON 包装 ──
  const wrapJsonParse = (orig) =>
    function parse(text, reviver) {
      const sessionShaped = isSessionShapedJsonLine(text);
      const t0 = process.hrtime.bigint();
      try {
        return orig.call(JSON, text, reviver);
      } finally {
        const ms = Number(process.hrtime.bigint() - t0) / 1e6;
        const scope = scopeOf();
        const g = scope ? scope.gauges : instance.global;
        g.parseMs += ms;
        if (sessionShaped) {
          g.jsonlParseCount += 1;
          g.jsonlParseMs += ms;
        }
      }
    };

  const wrapJsonStringify = (orig) =>
    function stringify(value, replacer, space) {
      const messagesResponse = isMessagesResponseObject(value);
      const t0 = process.hrtime.bigint();
      const result = orig.call(JSON, value, replacer, space);
      const ms = Number(process.hrtime.bigint() - t0) / 1e6;
      const scope = scopeOf();
      const g = scope ? scope.gauges : instance.global;
      if (messagesResponse) {
        g.serializeMs += ms;
        const bytes = Buffer.byteLength(String(result), "utf8");
        g.responseUtf8Bytes = bytes;
      }
      return result;
    };

  // ── install / uninstall ──
  const wrappers = {
    fsReadFileSync: wrapReadFileSync(fs.readFileSync),
    fsReadFile: wrapReadFileCb(fs.readFile),
    fsPromisesReadFile: wrapPromisesReadFileImpl(fsPromises.readFile),
    fsOpenSync: wrapOpenSync(fs.openSync),
    fsOpen: wrapOpenCb(fs.open),
    fsPromisesOpen: wrapPromisesOpen(fsPromises.open),
    fsReadSync: wrapReadSync(fs.readSync),
    fsCloseSync: wrapCloseSync(fs.closeSync),
    fsCreateReadStream: wrapCreateReadStream(fs.createReadStream),
    fsWriteFileSync: wrapWriteFileSync(fs.writeFileSync),
    fsCopyFileSync: wrapCopyFileSync(fs.copyFileSync),
    jsonParse: wrapJsonParse(JSON.parse),
    jsonStringify: wrapJsonStringify(JSON.stringify),
  };

  function install() {
    fs.readFileSync = wrappers.fsReadFileSync;
    installed.fsReadFileSync = true;
    fs.readFile = wrappers.fsReadFile;
    installed.fsReadFile = true;
    fsPromises.readFile = wrappers.fsPromisesReadFile;
    installed.fsPromisesReadFile = true;
    fs.openSync = wrappers.fsOpenSync;
    installed.fsOpenSync = true;
    fs.open = wrappers.fsOpen;
    installed.fsOpen = true;
    fsPromises.open = wrappers.fsPromisesOpen;
    fs.readSync = wrappers.fsReadSync;
    installed.fsReadSync = true;
    fs.closeSync = wrappers.fsCloseSync;
    fs.createReadStream = wrappers.fsCreateReadStream;
    installed.fsCreateReadStream = true;
    fs.writeFileSync = wrappers.fsWriteFileSync;
    installed.fsWriteFileSync = true;
    fs.copyFileSync = wrappers.fsCopyFileSync;
    installed.fsCopyFileSync = true;
    JSON.parse = wrappers.jsonParse;
    installed.json = true;
    JSON.stringify = wrappers.jsonStringify;
    activeCounters = instance;
  }

  function uninstall() {
    if (activeCounters !== instance) {
      throw new Error(`[${label}] uninstall：当前活动计数器不是本实例`);
    }
    if (fdEpisodes.size > 0) {
      // 未关闭句柄：如实报错，不吞——说明有请求结束但 fd 未关，归因不完整。
      const openFds = [...fdEpisodes.keys()];
      fdEpisodes.clear();
      throw new Error(`[${label}] uninstall 时仍有未关闭的 fd episode：${openFds.join(",")}`);
    }
    if (installed.fsReadFileSync) fs.readFileSync = originals.fs.readFileSync;
    if (installed.fsReadFile) fs.readFile = originals.fs.readFile;
    if (installed.fsPromisesReadFile) fsPromises.readFile = originals.fsPromises.readFile;
    if (installed.fsOpenSync) fs.openSync = originals.fs.openSync;
    if (installed.fsOpen) fs.open = originals.fs.open;
    if (installed.fsPromisesOpen) fsPromises.open = originals.fsPromises.open;
    if (installed.fsReadSync) fs.readSync = originals.fs.readSync;
    fs.closeSync = originals.fs.closeSync;
    if (installed.fsCreateReadStream) fs.createReadStream = originals.fs.createReadStream;
    if (installed.fsWriteFileSync) fs.writeFileSync = originals.fs.writeFileSync;
    if (installed.fsCopyFileSync) fs.copyFileSync = originals.fs.copyFileSync;
    if (installed.json) {
      JSON.parse = originals.jsonParse;
      JSON.stringify = originals.jsonStringify;
    }
    activeCounters = null;
  }

  // 保存原函数供恢复
  originals.fs.readFileSync = fs.readFileSync;
  originals.fs.readFile = fs.readFile;
  originals.fsPromises.readFile = fsPromises.readFile;
  originals.fs.openSync = fs.openSync;
  originals.fs.open = fs.open;
  originals.fsPromises.open = fsPromises.open;
  originals.fs.readSync = fs.readSync;
  originals.fs.closeSync = fs.closeSync;
  originals.fs.createReadStream = fs.createReadStream;
  originals.fs.writeFileSync = fs.writeFileSync;
  originals.fs.copyFileSync = fs.copyFileSync;

  return {
    label,
    install,
    uninstall,
    beginRequest: (meta) => instance.beginRequest(meta),
    endRequest: (handle) => instance.endRequest(handle),
    currentScope: () => instance.currentScope(),
    recordBranchOpen: (threw) => instance.recordBranchOpen(threw),
    recordGetBranch: (threw) => instance.recordGetBranch(threw),
    recordScan: (name, inputLength, ms) => instance.recordScan(name, inputLength, ms),
    snapshotOf: (gauges) => instance.snapshotOf(gauges),
    global: () => instance.global,
    resetGlobal: () => instance.reset(),
  };
}
