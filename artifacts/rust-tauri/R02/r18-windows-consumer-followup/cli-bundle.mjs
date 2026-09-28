#!/usr/bin/env node
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __require = /* @__PURE__ */ ((x) => typeof require !== "undefined" ? require : typeof Proxy !== "undefined" ? new Proxy(x, {
  get: (a, b) => (typeof require !== "undefined" ? require : a)[b]
}) : x)(function(x) {
  if (typeof require !== "undefined") return require.apply(this, arguments);
  throw Error('Dynamic require of "' + x + '" is not supported');
});
var __commonJS = (cb, mod) => function __require2() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// shared/hana-runtime-paths.cjs
var require_hana_runtime_paths = __commonJS({
  "shared/hana-runtime-paths.cjs"(exports, module) {
    "use strict";
    var os = __require("os");
    var path9 = __require("path");
    function expandHome(input, homeDir = os.homedir()) {
      if (!input) return input;
      if (input === "~") return homeDir;
      if (input.startsWith("~/") || input.startsWith("~" + path9.sep)) {
        return path9.join(homeDir, input.slice(2));
      }
      return input;
    }
    function resolveLingxiHome2(input, homeDir = os.homedir()) {
      const raw = input || path9.join(homeDir, ".lingxi");
      return path9.resolve(expandHome(raw, homeDir));
    }
    function assertLingxiHome(lingxiHome, caller) {
      if (!lingxiHome || typeof lingxiHome !== "string") {
        throw new Error(`${caller}: lingxiHome is required`);
      }
    }
    function resolveLingxiPiSdkRuntimeRoot2(lingxiHome) {
      assertLingxiHome(lingxiHome, "resolveLingxiPiSdkRuntimeRoot");
      return path9.join(lingxiHome, "runtime", "pi-sdk");
    }
    function resolveLingxiPiSdkManagedBinDir2(lingxiHome) {
      return path9.join(resolveLingxiPiSdkRuntimeRoot2(lingxiHome), "bin");
    }
    function resolveLingxiPiSdkResourceLoaderCwd2(lingxiHome) {
      return path9.join(resolveLingxiPiSdkRuntimeRoot2(lingxiHome), "resource-loader", "project");
    }
    function resolveLingxiPiSdkResourceLoaderAgentDir2(lingxiHome) {
      return path9.join(resolveLingxiPiSdkRuntimeRoot2(lingxiHome), "resource-loader", "agent");
    }
    module.exports = {
      resolveLingxiHome: resolveLingxiHome2,
      resolveLingxiPiSdkManagedBinDir: resolveLingxiPiSdkManagedBinDir2,
      resolveLingxiPiSdkResourceLoaderAgentDir: resolveLingxiPiSdkResourceLoaderAgentDir2,
      resolveLingxiPiSdkResourceLoaderCwd: resolveLingxiPiSdkResourceLoaderCwd2,
      resolveLingxiPiSdkRuntimeRoot: resolveLingxiPiSdkRuntimeRoot2
    };
  }
});

// shared/server-info-probe.cjs
var require_server_info_probe = __commonJS({
  "shared/server-info-probe.cjs"(exports, module) {
    "use strict";
    var DEFAULT_PROBE_PATH = "/api/server/identity";
    var DEFAULT_TIMEOUT_MS = 2e3;
    async function probeServerInfo3({ info, timeoutMs = DEFAULT_TIMEOUT_MS, fetchImpl, probePath = DEFAULT_PROBE_PATH } = {}) {
      const port = Number(info && info.port);
      const token = typeof (info && info.token) === "string" ? info.token : "";
      if (!Number.isInteger(port) || port <= 0 || !token) {
        return { status: "dead" };
      }
      const doFetch = fetchImpl || globalThis.fetch;
      if (typeof doFetch !== "function") {
        throw new Error("server-info-probe: no fetch implementation available (pass fetchImpl in this runtime)");
      }
      let res;
      try {
        res = await doFetch(`http://127.0.0.1:${port}${probePath}`, {
          headers: { Authorization: `Bearer ${token}` },
          signal: AbortSignal.timeout(timeoutMs)
        });
      } catch {
        return { status: "dead" };
      }
      let body = null;
      try {
        body = await res.json();
      } catch {
        body = null;
      }
      if (res.status === 200) {
        if (body && typeof body === "object" && typeof body.serverId === "string" && body.serverId) {
          return { status: "alive-same-home" };
        }
        return { status: "not-hana", detail: `200 response did not match the server-identity shape: ${safeDescribe(body)}` };
      }
      if (res.status === 403) {
        if (body && typeof body === "object" && (typeof body.reason === "string" || typeof body.error === "string")) {
          return { status: "alive-unauthorized" };
        }
        return { status: "not-hana", detail: `403 response did not match the auth-rejection shape: ${safeDescribe(body)}` };
      }
      return { status: "not-hana", detail: `unexpected HTTP status ${res.status}` };
    }
    function isForeignServerBlocking3(status) {
      return status === "alive-same-home" || status === "alive-unauthorized";
    }
    function describeForeignServerBlock3({ status, info }) {
      const ownerKind = info && info.ownerKind || "unknown";
      const version = info && info.version || "unknown";
      const pid = info && Number.isInteger(info.pid) ? info.pid : "unknown";
      if (status === "alive-same-home") {
        return `\u68C0\u6D4B\u5230\u540C\u4E00\u6570\u636E\u76EE\u5F55\u5DF2\u6709\u5185\u6838\u5728\u8FD0\u884C\uFF08ownerKind=${ownerKind}, version=${version}, pid=${pid}\uFF09\u3002\u8981\u63A5\u7BA1\u8BF7\u5148\u9000\u51FA\u5B83\uFF0C\u518D\u91CD\u65B0\u542F\u52A8\u3002
A LingxiAgent kernel is already running against this data directory (ownerKind=${ownerKind}, version=${version}, pid=${pid}). Quit it first, then start this one again.`;
      }
      if (status === "alive-unauthorized") {
        return `\u8BE5\u7AEF\u53E3\u4E0A\u6709\u4E00\u4E2A\u5185\u6838\u5728\u54CD\u5E94\uFF0C\u4F46\u65E0\u6CD5\u7528\u672C\u673A\u8BB0\u5F55\u7684\u51ED\u636E\u9A8C\u8BC1\u5B83\u7684\u8EAB\u4EFD\uFF08token \u53EF\u80FD\u5DF2\u8F6E\u6362\uFF0C\u6216\u53E6\u4E00\u4E2A Hana \u6570\u636E\u76EE\u5F55\u7684\u5185\u6838\u5360\u7528\u4E86\u8FD9\u4E2A\u7AEF\u53E3\uFF09\u3002\u8BF7\u5148\u6392\u67E5\uFF08ownerKind=${ownerKind}, pid=${pid}\uFF09\uFF0C\u786E\u8BA4\u5B89\u5168\u540E\u518D\u542F\u52A8\u3002
A kernel on that port responded but could not be authenticated with the credentials recorded locally (the token may have rotated, or a kernel from a different LINGXI_HOME is holding that port). Investigate first (ownerKind=${ownerKind}, pid=${pid}) before starting.`;
      }
      return null;
    }
    function safeDescribe(value) {
      try {
        return JSON.stringify(value);
      } catch {
        return String(value);
      }
    }
    module.exports = {
      DEFAULT_PROBE_PATH,
      DEFAULT_TIMEOUT_MS,
      probeServerInfo: probeServerInfo3,
      isForeignServerBlocking: isForeignServerBlocking3,
      describeForeignServerBlock: describeForeignServerBlock3
    };
  }
});

// shared/contract-versions.json
var require_contract_versions = __commonJS({
  "shared/contract-versions.json"(exports, module) {
    module.exports = {
      PRELOAD_API_VERSION: 1,
      SERVER_PROTOCOL_VERSION: 1,
      DATA_EPOCH: 1
    };
  }
});

// shared/contract-versions.cjs
var require_contract_versions2 = __commonJS({
  "shared/contract-versions.cjs"(exports, module) {
    "use strict";
    var { PRELOAD_API_VERSION, SERVER_PROTOCOL_VERSION: SERVER_PROTOCOL_VERSION2, DATA_EPOCH: DATA_EPOCH2 } = require_contract_versions();
    module.exports = {
      PRELOAD_API_VERSION,
      SERVER_PROTOCOL_VERSION: SERVER_PROTOCOL_VERSION2,
      DATA_EPOCH: DATA_EPOCH2
    };
  }
});

// shared/data-epoch.cjs
var require_data_epoch = __commonJS({
  "shared/data-epoch.cjs"(exports, module) {
    "use strict";
    var crypto4 = __require("crypto");
    var fs7 = __require("fs");
    var path9 = __require("path");
    var DATA_EPOCH_STAMP_SCHEMA_VERSION = 2;
    var DATA_EPOCH_JOURNAL_SCHEMA_VERSION = 1;
    var DATA_EPOCH_JOURNAL_PHASES = Object.freeze([
      "prepared",
      "checkpoint_complete",
      "barrier_raised",
      "migrating",
      "migrated",
      "validated",
      "committed"
    ]);
    function dataEpochStampPath2(homeDir) {
      return path9.join(homeDir, "data-epoch.json");
    }
    function dataEpochJournalPath2(homeDir) {
      return path9.join(homeDir, "data-epoch-transition.json");
    }
    function isPositiveInteger(value) {
      return Number.isInteger(value) && value >= 1;
    }
    function isTimestamp(value) {
      return typeof value === "string" && value.length > 0 && !Number.isNaN(Date.parse(value));
    }
    function corrupt(filePath, detail) {
      return { status: "corrupt", filePath, detail };
    }
    function readJsonFile(filePath) {
      let raw;
      try {
        raw = fs7.readFileSync(filePath, "utf8");
      } catch (error) {
        if (error?.code === "ENOENT") return { status: "missing", filePath };
        return corrupt(filePath, error instanceof Error ? error.message : String(error));
      }
      try {
        return { status: "present", filePath, value: JSON.parse(raw) };
      } catch (error) {
        return corrupt(filePath, error instanceof Error ? error.message : String(error));
      }
    }
    function readDataEpochStamp3(homeDir) {
      const filePath = dataEpochStampPath2(homeDir);
      const read = readJsonFile(filePath);
      if (read.status !== "present") return read;
      const value = read.value;
      if (!value || typeof value !== "object" || Array.isArray(value)) {
        return corrupt(filePath, "stamp must be a JSON object");
      }
      if (value.schemaVersion === void 0) {
        if (!isPositiveInteger(value.epoch)) {
          return corrupt(filePath, "legacy stamp is missing a positive integer `epoch`");
        }
        if (value.lastVersion !== void 0 && typeof value.lastVersion !== "string") {
          return corrupt(filePath, "legacy stamp has an invalid `lastVersion`");
        }
        if (value.updatedAt !== void 0 && !isTimestamp(value.updatedAt)) {
          return corrupt(filePath, "legacy stamp has an invalid `updatedAt`");
        }
        return {
          status: "ok",
          filePath,
          format: "legacy-v1",
          stamp: {
            schemaVersion: 1,
            epoch: value.epoch,
            minimumReaderEpoch: value.epoch,
            committedDataEpoch: value.epoch,
            lastVersion: typeof value.lastVersion === "string" ? value.lastVersion : null,
            updatedAt: typeof value.updatedAt === "string" ? value.updatedAt : null
          }
        };
      }
      if (value.schemaVersion !== DATA_EPOCH_STAMP_SCHEMA_VERSION) {
        return corrupt(filePath, `unsupported stamp schemaVersion: ${String(value.schemaVersion)}`);
      }
      if (!isPositiveInteger(value.epoch) || !isPositiveInteger(value.minimumReaderEpoch)) {
        return corrupt(filePath, "v2 stamp requires positive integer `epoch` and `minimumReaderEpoch`");
      }
      if (value.epoch !== value.minimumReaderEpoch) {
        return corrupt(filePath, "v2 stamp requires `epoch` to equal `minimumReaderEpoch`");
      }
      if (!isPositiveInteger(value.committedDataEpoch)) {
        return corrupt(filePath, "v2 stamp requires a positive integer `committedDataEpoch`");
      }
      if (value.committedDataEpoch > value.minimumReaderEpoch) {
        return corrupt(filePath, "v2 stamp cannot commit a higher epoch than its minimum reader barrier");
      }
      if (typeof value.lastVersion !== "string" || value.lastVersion.length === 0) {
        return corrupt(filePath, "v2 stamp requires a non-empty `lastVersion`");
      }
      if (!isTimestamp(value.updatedAt)) {
        return corrupt(filePath, "v2 stamp requires a valid `updatedAt`");
      }
      return {
        status: "ok",
        filePath,
        format: "v2",
        stamp: {
          schemaVersion: DATA_EPOCH_STAMP_SCHEMA_VERSION,
          epoch: value.epoch,
          minimumReaderEpoch: value.minimumReaderEpoch,
          committedDataEpoch: value.committedDataEpoch,
          lastVersion: value.lastVersion,
          updatedAt: value.updatedAt
        }
      };
    }
    function readDataEpochJournal5(homeDir) {
      const filePath = dataEpochJournalPath2(homeDir);
      const read = readJsonFile(filePath);
      if (read.status !== "present") return read;
      const value = read.value;
      if (!value || typeof value !== "object" || Array.isArray(value)) {
        return corrupt(filePath, "transition journal must be a JSON object");
      }
      if (value.schemaVersion !== DATA_EPOCH_JOURNAL_SCHEMA_VERSION) {
        return corrupt(filePath, `unsupported transition journal schemaVersion: ${String(value.schemaVersion)}`);
      }
      if (typeof value.transitionId !== "string" || value.transitionId.length === 0) {
        return corrupt(filePath, "transition journal requires a non-empty transitionId");
      }
      if (!isPositiveInteger(value.fromEpoch) || !isPositiveInteger(value.toEpoch) || value.fromEpoch >= value.toEpoch) {
        return corrupt(filePath, "transition journal requires fromEpoch < toEpoch");
      }
      if (!DATA_EPOCH_JOURNAL_PHASES.includes(value.phase)) {
        return corrupt(filePath, `transition journal has an invalid phase: ${String(value.phase)}`);
      }
      if (!Array.isArray(value.migrationIds) || value.migrationIds.length === 0 || value.migrationIds.some((id) => typeof id !== "string" || id.length === 0) || new Set(value.migrationIds).size !== value.migrationIds.length) {
        return corrupt(filePath, "transition journal requires unique migrationIds");
      }
      if (!value.recoveryModes || typeof value.recoveryModes !== "object" || Array.isArray(value.recoveryModes)) {
        return corrupt(filePath, "transition journal requires recoveryModes");
      }
      const recoveryModeKeys = Object.keys(value.recoveryModes).sort();
      const migrationIds = [...value.migrationIds];
      if (JSON.stringify(recoveryModeKeys) !== JSON.stringify([...migrationIds].sort()) || recoveryModeKeys.some((id) => !["resume-idempotent", "restore-only"].includes(value.recoveryModes[id]))) {
        return corrupt(filePath, "transition journal recoveryModes must exactly cover migrationIds");
      }
      if (!Array.isArray(value.affectedStoreIds) || value.affectedStoreIds.length === 0 || value.affectedStoreIds.some((id) => typeof id !== "string" || id.length === 0) || new Set(value.affectedStoreIds).size !== value.affectedStoreIds.length) {
        return corrupt(filePath, "transition journal requires unique affectedStoreIds");
      }
      if (typeof value.lastVersion !== "string" || value.lastVersion.length === 0) {
        return corrupt(filePath, "transition journal requires a non-empty lastVersion");
      }
      if (!isTimestamp(value.createdAt) || !isTimestamp(value.updatedAt)) {
        return corrupt(filePath, "transition journal requires valid timestamps");
      }
      const checkpointRequired = value.phase !== "prepared";
      if (checkpointRequired) {
        if (typeof value.checkpointId !== "string" || value.checkpointId.length === 0 || !value.checkpointReceipt || typeof value.checkpointReceipt !== "object" || Array.isArray(value.checkpointReceipt) || value.checkpointReceipt.id !== value.checkpointId) {
          return corrupt(filePath, `transition journal phase ${value.phase} requires a checkpoint receipt`);
        }
      } else if (value.checkpointId !== null || value.checkpointReceipt !== null) {
        return corrupt(filePath, "prepared transition journal must not claim a completed checkpoint");
      }
      return {
        status: "ok",
        filePath,
        journal: {
          schemaVersion: DATA_EPOCH_JOURNAL_SCHEMA_VERSION,
          transitionId: value.transitionId,
          fromEpoch: value.fromEpoch,
          toEpoch: value.toEpoch,
          migrationIds,
          phase: value.phase,
          recoveryModes: { ...value.recoveryModes },
          lastVersion: value.lastVersion,
          createdAt: value.createdAt,
          updatedAt: value.updatedAt,
          affectedStoreIds: [...value.affectedStoreIds],
          checkpointId: value.checkpointId,
          checkpointReceipt: value.checkpointReceipt
        }
      };
    }
    async function syncParentDirectory(filePath) {
      if (process.platform === "win32") return;
      const handle = await fs7.promises.open(path9.dirname(filePath), "r");
      try {
        await handle.sync();
      } finally {
        await handle.close();
      }
    }
    async function durableWriteJson2(filePath, value) {
      await fs7.promises.mkdir(path9.dirname(filePath), { recursive: true });
      const temporaryPath = `${filePath}.tmp-${process.pid}-${crypto4.randomBytes(8).toString("hex")}`;
      const serialized = `${JSON.stringify(value, null, 2)}
`;
      let handle = null;
      try {
        handle = await fs7.promises.open(temporaryPath, "wx");
        await handle.writeFile(serialized, "utf8");
        await handle.sync();
        await handle.close();
        handle = null;
        await fs7.promises.rename(temporaryPath, filePath);
        await syncParentDirectory(filePath);
      } catch (error) {
        if (handle) await handle.close().catch(() => {
        });
        await fs7.promises.unlink(temporaryPath).catch(() => {
        });
        throw error;
      }
    }
    function createDataEpochStamp({ minimumReaderEpoch, committedDataEpoch, lastVersion, updatedAt = (/* @__PURE__ */ new Date()).toISOString() }) {
      if (!isPositiveInteger(minimumReaderEpoch) || !isPositiveInteger(committedDataEpoch)) {
        throw new Error("data epoch stamp requires positive integer epochs");
      }
      if (committedDataEpoch > minimumReaderEpoch) {
        throw new Error("committedDataEpoch cannot exceed minimumReaderEpoch");
      }
      if (typeof lastVersion !== "string" || lastVersion.length === 0) {
        throw new Error("data epoch stamp requires lastVersion");
      }
      if (!isTimestamp(updatedAt)) throw new Error("data epoch stamp requires a valid updatedAt timestamp");
      return {
        schemaVersion: DATA_EPOCH_STAMP_SCHEMA_VERSION,
        epoch: minimumReaderEpoch,
        minimumReaderEpoch,
        committedDataEpoch,
        lastVersion,
        updatedAt
      };
    }
    async function writeDataEpochStamp2(homeDir, input) {
      const stamp = createDataEpochStamp(input);
      await durableWriteJson2(dataEpochStampPath2(homeDir), stamp);
      return stamp;
    }
    function createDataEpochJournal(input) {
      const now = input.updatedAt ?? (/* @__PURE__ */ new Date()).toISOString();
      const journal = {
        schemaVersion: DATA_EPOCH_JOURNAL_SCHEMA_VERSION,
        transitionId: input.transitionId,
        fromEpoch: input.fromEpoch,
        toEpoch: input.toEpoch,
        migrationIds: [...input.migrationIds],
        affectedStoreIds: [...input.affectedStoreIds],
        recoveryModes: { ...input.recoveryModes },
        phase: input.phase,
        checkpointId: input.checkpointId ?? null,
        checkpointReceipt: input.checkpointReceipt ?? null,
        createdAt: input.createdAt ?? now,
        updatedAt: now,
        lastVersion: input.lastVersion
      };
      const validation = readJournalValueForValidation(journal);
      if (validation !== null) throw new Error(validation);
      return journal;
    }
    function readJournalValueForValidation(value) {
      if (typeof value.transitionId !== "string" || value.transitionId.length === 0) return "transition journal requires transitionId";
      if (!isPositiveInteger(value.fromEpoch) || !isPositiveInteger(value.toEpoch) || value.fromEpoch >= value.toEpoch) {
        return "transition journal requires fromEpoch < toEpoch";
      }
      if (!DATA_EPOCH_JOURNAL_PHASES.includes(value.phase)) return `invalid transition journal phase: ${String(value.phase)}`;
      if (!Array.isArray(value.migrationIds) || value.migrationIds.length === 0 || value.migrationIds.some((id) => typeof id !== "string" || id.length === 0) || new Set(value.migrationIds).size !== value.migrationIds.length) {
        return "transition journal requires unique migrationIds";
      }
      if (!Array.isArray(value.affectedStoreIds) || value.affectedStoreIds.length === 0 || value.affectedStoreIds.some((id) => typeof id !== "string" || id.length === 0) || new Set(value.affectedStoreIds).size !== value.affectedStoreIds.length) {
        return "transition journal requires unique affectedStoreIds";
      }
      if (!value.recoveryModes || typeof value.recoveryModes !== "object" || Array.isArray(value.recoveryModes) || JSON.stringify(Object.keys(value.recoveryModes).sort()) !== JSON.stringify([...value.migrationIds].sort()) || Object.values(value.recoveryModes).some((mode) => mode !== "resume-idempotent" && mode !== "restore-only")) {
        return "transition journal recoveryModes must exactly cover migrationIds";
      }
      if (typeof value.lastVersion !== "string" || value.lastVersion.length === 0) return "transition journal requires lastVersion";
      if (!isTimestamp(value.createdAt) || !isTimestamp(value.updatedAt)) return "transition journal requires valid timestamps";
      if (value.phase === "prepared" && (value.checkpointId !== null || value.checkpointReceipt !== null)) {
        return "prepared transition journal cannot contain a checkpoint";
      }
      if (value.phase !== "prepared" && (typeof value.checkpointId !== "string" || value.checkpointId.length === 0 || !value.checkpointReceipt || typeof value.checkpointReceipt !== "object" || Array.isArray(value.checkpointReceipt) || value.checkpointReceipt.id !== value.checkpointId)) {
        return `transition journal phase ${value.phase} requires a checkpoint receipt`;
      }
      return null;
    }
    async function writeDataEpochJournal2(homeDir, input) {
      const journal = createDataEpochJournal(input);
      await durableWriteJson2(dataEpochJournalPath2(homeDir), journal);
      return journal;
    }
    async function removeDataEpochJournal3(homeDir) {
      const filePath = dataEpochJournalPath2(homeDir);
      try {
        await fs7.promises.unlink(filePath);
      } catch (error) {
        if (error?.code === "ENOENT") return false;
        throw error;
      }
      await syncParentDirectory(filePath);
      return true;
    }
    var DATA_EPOCH_RESTORE_JOURNAL_SCHEMA_VERSION = 1;
    var DATA_EPOCH_RESTORE_JOURNAL_PHASES = Object.freeze([
      "restore:starting",
      "restore:stores_restored",
      "restore:metadata_republished"
    ]);
    function restoreJournalValidationProblem(value) {
      if (value.kind !== "restore") return 'restore journal requires kind "restore"';
      if (value.restoreSchemaVersion !== DATA_EPOCH_RESTORE_JOURNAL_SCHEMA_VERSION) {
        return `unsupported restore journal restoreSchemaVersion: ${String(value.restoreSchemaVersion)}`;
      }
      if (typeof value.restoreId !== "string" || value.restoreId.length === 0) {
        return "restore journal requires a non-empty restoreId";
      }
      if (typeof value.transitionId !== "string" || value.transitionId.length === 0) {
        return "restore journal requires a non-empty transitionId";
      }
      if (!isPositiveInteger(value.fromEpoch)) {
        return "restore journal requires a positive integer fromEpoch";
      }
      if (!DATA_EPOCH_RESTORE_JOURNAL_PHASES.includes(value.phase)) {
        return `invalid restore journal phase: ${String(value.phase)}`;
      }
      if (!isTimestamp(value.createdAt) || !isTimestamp(value.updatedAt)) {
        return "restore journal requires valid timestamps";
      }
      return null;
    }
    function readDataEpochRestoreJournal3(homeDir) {
      const filePath = dataEpochJournalPath2(homeDir);
      const read = readJsonFile(filePath);
      if (read.status !== "present") return read;
      const value = read.value;
      if (!value || typeof value !== "object" || Array.isArray(value)) {
        return corrupt(filePath, "transition journal must be a JSON object");
      }
      const problem = restoreJournalValidationProblem(value);
      if (problem !== null) return corrupt(filePath, problem);
      return {
        status: "ok",
        filePath,
        journal: {
          kind: "restore",
          restoreSchemaVersion: DATA_EPOCH_RESTORE_JOURNAL_SCHEMA_VERSION,
          restoreId: value.restoreId,
          transitionId: value.transitionId,
          fromEpoch: value.fromEpoch,
          phase: value.phase,
          createdAt: value.createdAt,
          updatedAt: value.updatedAt
        }
      };
    }
    function createDataEpochRestoreJournal(input) {
      const now = input.updatedAt ?? (/* @__PURE__ */ new Date()).toISOString();
      const journal = {
        kind: "restore",
        restoreSchemaVersion: DATA_EPOCH_RESTORE_JOURNAL_SCHEMA_VERSION,
        restoreId: input.restoreId,
        transitionId: input.transitionId,
        fromEpoch: input.fromEpoch,
        phase: input.phase,
        createdAt: input.createdAt ?? now,
        updatedAt: now
      };
      const problem = restoreJournalValidationProblem(journal);
      if (problem !== null) throw new Error(problem);
      return journal;
    }
    async function writeDataEpochRestoreJournal2(homeDir, input) {
      const journal = createDataEpochRestoreJournal(input);
      await durableWriteJson2(dataEpochJournalPath2(homeDir), journal);
      return journal;
    }
    async function republishDataEpochStampForRestore2({ homeDir, fromEpoch, lastVersion, updatedAt } = {}) {
      const restoreJournalRead = readDataEpochRestoreJournal3(homeDir);
      const restorePhasesAllowingRepublish = ["restore:stores_restored", "restore:metadata_republished"];
      if (restoreJournalRead.status !== "ok" || !restorePhasesAllowingRepublish.includes(restoreJournalRead.journal.phase)) {
        throw new Error(
          "republishDataEpochStampForRestore requires an on-disk restore journal that has finished restoring store bytes; this is the only channel allowed to lower the data epoch stamp"
        );
      }
      if (restoreJournalRead.journal.fromEpoch !== fromEpoch) {
        throw new Error(
          `republishDataEpochStampForRestore: the restore journal targets fromEpoch=${restoreJournalRead.journal.fromEpoch}, but was called with fromEpoch=${String(fromEpoch)}`
        );
      }
      const stamp = createDataEpochStamp({ minimumReaderEpoch: fromEpoch, committedDataEpoch: fromEpoch, lastVersion, updatedAt });
      await durableWriteJson2(dataEpochStampPath2(homeDir), stamp);
      return stamp;
    }
    function describeDataEpochBlock2({ stampEpoch, ownEpoch, stampLastVersion }) {
      const lastVersionNote = stampLastVersion ? ` (last opened by version ${stampLastVersion})` : "";
      return `\u6B64\u6570\u636E\u76EE\u5F55\u8981\u6C42\u6570\u636E epoch=${stampEpoch} \u6216\u66F4\u9AD8\u7248\u672C\u7684\u5185\u6838${lastVersionNote}\uFF0C\u672C\u5185\u6838 epoch=${ownEpoch}\u3002\u7EE7\u7EED\u4F7F\u7528\u65E7\u5185\u6838\u53EF\u80FD\u9759\u9ED8\u635F\u574F\u6570\u636E\u3002\u8BF7\u5347\u7EA7\u5230\u8F83\u65B0\u7248\u672C\uFF0C\u6216\u5728\u786E\u8BA4\u98CE\u9669\u540E\u8BBE\u7F6E LINGXI_ALLOW_DATA_DOWNGRADE=1\uFF08\u6216\u5BF9 hana serve \u4F20 --allow-data-downgrade\uFF09\u3002
This data directory requires a kernel at data epoch=${stampEpoch} or newer${lastVersionNote}; this kernel is epoch=${ownEpoch}. Continuing with an older kernel risks silent corruption. Upgrade, or explicitly accept the risk with LINGXI_ALLOW_DATA_DOWNGRADE=1 (or --allow-data-downgrade for hana serve).`;
    }
    module.exports = {
      DATA_EPOCH_STAMP_SCHEMA_VERSION,
      DATA_EPOCH_JOURNAL_SCHEMA_VERSION,
      DATA_EPOCH_JOURNAL_PHASES,
      dataEpochStampPath: dataEpochStampPath2,
      dataEpochJournalPath: dataEpochJournalPath2,
      readDataEpochStamp: readDataEpochStamp3,
      readDataEpochJournal: readDataEpochJournal5,
      durableWriteJson: durableWriteJson2,
      createDataEpochStamp,
      writeDataEpochStamp: writeDataEpochStamp2,
      createDataEpochJournal,
      writeDataEpochJournal: writeDataEpochJournal2,
      removeDataEpochJournal: removeDataEpochJournal3,
      describeDataEpochBlock: describeDataEpochBlock2,
      DATA_EPOCH_RESTORE_JOURNAL_SCHEMA_VERSION,
      DATA_EPOCH_RESTORE_JOURNAL_PHASES,
      readDataEpochRestoreJournal: readDataEpochRestoreJournal3,
      createDataEpochRestoreJournal,
      writeDataEpochRestoreJournal: writeDataEpochRestoreJournal2,
      republishDataEpochStampForRestore: republishDataEpochStampForRestore2
    };
  }
});

// node_modules/better-sqlite3/lib/util.js
var require_util = __commonJS({
  "node_modules/better-sqlite3/lib/util.js"(exports) {
    "use strict";
    exports.getBooleanOption = (options, key) => {
      let value = false;
      if (key in options && typeof (value = options[key]) !== "boolean") {
        throw new TypeError(`Expected the "${key}" option to be a boolean`);
      }
      return value;
    };
    exports.cppdb = /* @__PURE__ */ Symbol();
    exports.inspect = /* @__PURE__ */ Symbol.for("nodejs.util.inspect.custom");
  }
});

// node_modules/better-sqlite3/lib/sqlite-error.js
var require_sqlite_error = __commonJS({
  "node_modules/better-sqlite3/lib/sqlite-error.js"(exports, module) {
    "use strict";
    var descriptor = { value: "SqliteError", writable: true, enumerable: false, configurable: true };
    function SqliteError(message, code) {
      if (new.target !== SqliteError) {
        return new SqliteError(message, code);
      }
      if (typeof code !== "string") {
        throw new TypeError("Expected second argument to be a string");
      }
      Error.call(this, message);
      descriptor.value = "" + message;
      Object.defineProperty(this, "message", descriptor);
      Error.captureStackTrace(this, SqliteError);
      this.code = code;
    }
    Object.setPrototypeOf(SqliteError, Error);
    Object.setPrototypeOf(SqliteError.prototype, Error.prototype);
    Object.defineProperty(SqliteError.prototype, "name", descriptor);
    module.exports = SqliteError;
  }
});

// node_modules/file-uri-to-path/index.js
var require_file_uri_to_path = __commonJS({
  "node_modules/file-uri-to-path/index.js"(exports, module) {
    var sep = __require("path").sep || "/";
    module.exports = fileUriToPath;
    function fileUriToPath(uri) {
      if ("string" != typeof uri || uri.length <= 7 || "file://" != uri.substring(0, 7)) {
        throw new TypeError("must pass in a file:// URI to convert to a file path");
      }
      var rest = decodeURI(uri.substring(7));
      var firstSlash = rest.indexOf("/");
      var host = rest.substring(0, firstSlash);
      var path9 = rest.substring(firstSlash + 1);
      if ("localhost" == host) host = "";
      if (host) {
        host = sep + sep + host;
      }
      path9 = path9.replace(/^(.+)\|/, "$1:");
      if (sep == "\\") {
        path9 = path9.replace(/\//g, "\\");
      }
      if (/^.+\:/.test(path9)) {
      } else {
        path9 = sep + path9;
      }
      return host + path9;
    }
  }
});

// node_modules/bindings/bindings.js
var require_bindings = __commonJS({
  "node_modules/bindings/bindings.js"(exports, module) {
    var fs7 = __require("fs");
    var path9 = __require("path");
    var fileURLToPath4 = require_file_uri_to_path();
    var join = path9.join;
    var dirname = path9.dirname;
    var exists = fs7.accessSync && function(path10) {
      try {
        fs7.accessSync(path10);
      } catch (e) {
        return false;
      }
      return true;
    } || fs7.existsSync || path9.existsSync;
    var defaults = {
      arrow: process.env.NODE_BINDINGS_ARROW || " \u2192 ",
      compiled: process.env.NODE_BINDINGS_COMPILED_DIR || "compiled",
      platform: process.platform,
      arch: process.arch,
      nodePreGyp: "node-v" + process.versions.modules + "-" + process.platform + "-" + process.arch,
      version: process.versions.node,
      bindings: "bindings.node",
      try: [
        // node-gyp's linked version in the "build" dir
        ["module_root", "build", "bindings"],
        // node-waf and gyp_addon (a.k.a node-gyp)
        ["module_root", "build", "Debug", "bindings"],
        ["module_root", "build", "Release", "bindings"],
        // Debug files, for development (legacy behavior, remove for node v0.9)
        ["module_root", "out", "Debug", "bindings"],
        ["module_root", "Debug", "bindings"],
        // Release files, but manually compiled (legacy behavior, remove for node v0.9)
        ["module_root", "out", "Release", "bindings"],
        ["module_root", "Release", "bindings"],
        // Legacy from node-waf, node <= 0.4.x
        ["module_root", "build", "default", "bindings"],
        // Production "Release" buildtype binary (meh...)
        ["module_root", "compiled", "version", "platform", "arch", "bindings"],
        // node-qbs builds
        ["module_root", "addon-build", "release", "install-root", "bindings"],
        ["module_root", "addon-build", "debug", "install-root", "bindings"],
        ["module_root", "addon-build", "default", "install-root", "bindings"],
        // node-pre-gyp path ./lib/binding/{node_abi}-{platform}-{arch}
        ["module_root", "lib", "binding", "nodePreGyp", "bindings"]
      ]
    };
    function bindings(opts) {
      if (typeof opts == "string") {
        opts = { bindings: opts };
      } else if (!opts) {
        opts = {};
      }
      Object.keys(defaults).map(function(i2) {
        if (!(i2 in opts)) opts[i2] = defaults[i2];
      });
      if (!opts.module_root) {
        opts.module_root = exports.getRoot(exports.getFileName());
      }
      if (path9.extname(opts.bindings) != ".node") {
        opts.bindings += ".node";
      }
      var requireFunc = typeof __webpack_require__ === "function" ? __non_webpack_require__ : __require;
      var tries = [], i = 0, l = opts.try.length, n, b, err;
      for (; i < l; i++) {
        n = join.apply(
          null,
          opts.try[i].map(function(p) {
            return opts[p] || p;
          })
        );
        tries.push(n);
        try {
          b = opts.path ? requireFunc.resolve(n) : requireFunc(n);
          if (!opts.path) {
            b.path = n;
          }
          return b;
        } catch (e) {
          if (e.code !== "MODULE_NOT_FOUND" && e.code !== "QUALIFIED_PATH_RESOLUTION_FAILED" && !/not find/i.test(e.message)) {
            throw e;
          }
        }
      }
      err = new Error(
        "Could not locate the bindings file. Tried:\n" + tries.map(function(a) {
          return opts.arrow + a;
        }).join("\n")
      );
      err.tries = tries;
      throw err;
    }
    module.exports = exports = bindings;
    exports.getFileName = function getFileName(calling_file) {
      var origPST = Error.prepareStackTrace, origSTL = Error.stackTraceLimit, dummy = {}, fileName;
      Error.stackTraceLimit = 10;
      Error.prepareStackTrace = function(e, st) {
        for (var i = 0, l = st.length; i < l; i++) {
          fileName = st[i].getFileName();
          if (fileName !== __filename) {
            if (calling_file) {
              if (fileName !== calling_file) {
                return;
              }
            } else {
              return;
            }
          }
        }
      };
      Error.captureStackTrace(dummy);
      dummy.stack;
      Error.prepareStackTrace = origPST;
      Error.stackTraceLimit = origSTL;
      var fileSchema = "file://";
      if (fileName.indexOf(fileSchema) === 0) {
        fileName = fileURLToPath4(fileName);
      }
      return fileName;
    };
    exports.getRoot = function getRoot(file) {
      var dir = dirname(file), prev;
      while (true) {
        if (dir === ".") {
          dir = process.cwd();
        }
        if (exists(join(dir, "package.json")) || exists(join(dir, "node_modules"))) {
          return dir;
        }
        if (prev === dir) {
          throw new Error(
            'Could not find module root given file: "' + file + '". Do you have a `package.json` file? '
          );
        }
        prev = dir;
        dir = join(dir, "..");
      }
    };
  }
});

// node_modules/better-sqlite3/lib/methods/wrappers.js
var require_wrappers = __commonJS({
  "node_modules/better-sqlite3/lib/methods/wrappers.js"(exports) {
    "use strict";
    var { cppdb } = require_util();
    exports.prepare = function prepare(sql) {
      return this[cppdb].prepare(sql, this, false);
    };
    exports.exec = function exec(sql) {
      this[cppdb].exec(sql);
      return this;
    };
    exports.close = function close() {
      this[cppdb].close();
      return this;
    };
    exports.loadExtension = function loadExtension(...args) {
      this[cppdb].loadExtension(...args);
      return this;
    };
    exports.defaultSafeIntegers = function defaultSafeIntegers(...args) {
      this[cppdb].defaultSafeIntegers(...args);
      return this;
    };
    exports.unsafeMode = function unsafeMode(...args) {
      this[cppdb].unsafeMode(...args);
      return this;
    };
    exports.getters = {
      name: {
        get: function name() {
          return this[cppdb].name;
        },
        enumerable: true
      },
      open: {
        get: function open() {
          return this[cppdb].open;
        },
        enumerable: true
      },
      inTransaction: {
        get: function inTransaction() {
          return this[cppdb].inTransaction;
        },
        enumerable: true
      },
      readonly: {
        get: function readonly() {
          return this[cppdb].readonly;
        },
        enumerable: true
      },
      memory: {
        get: function memory() {
          return this[cppdb].memory;
        },
        enumerable: true
      }
    };
  }
});

// node_modules/better-sqlite3/lib/methods/transaction.js
var require_transaction = __commonJS({
  "node_modules/better-sqlite3/lib/methods/transaction.js"(exports, module) {
    "use strict";
    var { cppdb } = require_util();
    var controllers = /* @__PURE__ */ new WeakMap();
    module.exports = function transaction(fn) {
      if (typeof fn !== "function") throw new TypeError("Expected first argument to be a function");
      const db = this[cppdb];
      const controller = getController(db, this);
      const { apply } = Function.prototype;
      const properties = {
        default: { value: wrapTransaction(apply, fn, db, controller.default) },
        deferred: { value: wrapTransaction(apply, fn, db, controller.deferred) },
        immediate: { value: wrapTransaction(apply, fn, db, controller.immediate) },
        exclusive: { value: wrapTransaction(apply, fn, db, controller.exclusive) },
        database: { value: this, enumerable: true }
      };
      Object.defineProperties(properties.default.value, properties);
      Object.defineProperties(properties.deferred.value, properties);
      Object.defineProperties(properties.immediate.value, properties);
      Object.defineProperties(properties.exclusive.value, properties);
      return properties.default.value;
    };
    var getController = (db, self) => {
      let controller = controllers.get(db);
      if (!controller) {
        const shared = {
          commit: db.prepare("COMMIT", self, false),
          rollback: db.prepare("ROLLBACK", self, false),
          savepoint: db.prepare("SAVEPOINT `	_bs3.	`", self, false),
          release: db.prepare("RELEASE `	_bs3.	`", self, false),
          rollbackTo: db.prepare("ROLLBACK TO `	_bs3.	`", self, false)
        };
        controllers.set(db, controller = {
          default: Object.assign({ begin: db.prepare("BEGIN", self, false) }, shared),
          deferred: Object.assign({ begin: db.prepare("BEGIN DEFERRED", self, false) }, shared),
          immediate: Object.assign({ begin: db.prepare("BEGIN IMMEDIATE", self, false) }, shared),
          exclusive: Object.assign({ begin: db.prepare("BEGIN EXCLUSIVE", self, false) }, shared)
        });
      }
      return controller;
    };
    var wrapTransaction = (apply, fn, db, { begin, commit, rollback, savepoint, release, rollbackTo }) => function sqliteTransaction() {
      let before, after, undo;
      if (db.inTransaction) {
        before = savepoint;
        after = release;
        undo = rollbackTo;
      } else {
        before = begin;
        after = commit;
        undo = rollback;
      }
      before.run();
      try {
        const result = apply.call(fn, this, arguments);
        if (result && typeof result.then === "function") {
          throw new TypeError("Transaction function cannot return a promise");
        }
        after.run();
        return result;
      } catch (ex) {
        if (db.inTransaction) {
          undo.run();
          if (undo !== rollback) after.run();
        }
        throw ex;
      }
    };
  }
});

// node_modules/better-sqlite3/lib/methods/pragma.js
var require_pragma = __commonJS({
  "node_modules/better-sqlite3/lib/methods/pragma.js"(exports, module) {
    "use strict";
    var { getBooleanOption, cppdb } = require_util();
    module.exports = function pragma(source, options) {
      if (options == null) options = {};
      if (typeof source !== "string") throw new TypeError("Expected first argument to be a string");
      if (typeof options !== "object") throw new TypeError("Expected second argument to be an options object");
      const simple = getBooleanOption(options, "simple");
      const stmt = this[cppdb].prepare(`PRAGMA ${source}`, this, true);
      return simple ? stmt.pluck().get() : stmt.all();
    };
  }
});

// node_modules/better-sqlite3/lib/methods/backup.js
var require_backup = __commonJS({
  "node_modules/better-sqlite3/lib/methods/backup.js"(exports, module) {
    "use strict";
    var fs7 = __require("fs");
    var path9 = __require("path");
    var { promisify } = __require("util");
    var { cppdb } = require_util();
    var fsAccess = promisify(fs7.access);
    module.exports = async function backup(filename, options) {
      if (options == null) options = {};
      if (typeof filename !== "string") throw new TypeError("Expected first argument to be a string");
      if (typeof options !== "object") throw new TypeError("Expected second argument to be an options object");
      filename = filename.trim();
      const attachedName = "attached" in options ? options.attached : "main";
      const handler = "progress" in options ? options.progress : null;
      if (!filename) throw new TypeError("Backup filename cannot be an empty string");
      if (filename === ":memory:") throw new TypeError('Invalid backup filename ":memory:"');
      if (typeof attachedName !== "string") throw new TypeError('Expected the "attached" option to be a string');
      if (!attachedName) throw new TypeError('The "attached" option cannot be an empty string');
      if (handler != null && typeof handler !== "function") throw new TypeError('Expected the "progress" option to be a function');
      await fsAccess(path9.dirname(filename)).catch(() => {
        throw new TypeError("Cannot save backup because the directory does not exist");
      });
      const isNewFile = await fsAccess(filename).then(() => false, () => true);
      return runBackup(this[cppdb].backup(this, attachedName, filename, isNewFile), handler || null);
    };
    var runBackup = (backup, handler) => {
      let rate = 0;
      let useDefault = true;
      return new Promise((resolve, reject) => {
        setImmediate(function step() {
          try {
            const progress = backup.transfer(rate);
            if (!progress.remainingPages) {
              backup.close();
              resolve(progress);
              return;
            }
            if (useDefault) {
              useDefault = false;
              rate = 100;
            }
            if (handler) {
              const ret = handler(progress);
              if (ret !== void 0) {
                if (typeof ret === "number" && ret === ret) rate = Math.max(0, Math.min(2147483647, Math.round(ret)));
                else throw new TypeError("Expected progress callback to return a number or undefined");
              }
            }
            setImmediate(step);
          } catch (err) {
            backup.close();
            reject(err);
          }
        });
      });
    };
  }
});

// node_modules/better-sqlite3/lib/methods/serialize.js
var require_serialize = __commonJS({
  "node_modules/better-sqlite3/lib/methods/serialize.js"(exports, module) {
    "use strict";
    var { cppdb } = require_util();
    module.exports = function serialize(options) {
      if (options == null) options = {};
      if (typeof options !== "object") throw new TypeError("Expected first argument to be an options object");
      const attachedName = "attached" in options ? options.attached : "main";
      if (typeof attachedName !== "string") throw new TypeError('Expected the "attached" option to be a string');
      if (!attachedName) throw new TypeError('The "attached" option cannot be an empty string');
      return this[cppdb].serialize(attachedName);
    };
  }
});

// node_modules/better-sqlite3/lib/methods/function.js
var require_function = __commonJS({
  "node_modules/better-sqlite3/lib/methods/function.js"(exports, module) {
    "use strict";
    var { getBooleanOption, cppdb } = require_util();
    module.exports = function defineFunction(name, options, fn) {
      if (options == null) options = {};
      if (typeof options === "function") {
        fn = options;
        options = {};
      }
      if (typeof name !== "string") throw new TypeError("Expected first argument to be a string");
      if (typeof fn !== "function") throw new TypeError("Expected last argument to be a function");
      if (typeof options !== "object") throw new TypeError("Expected second argument to be an options object");
      if (!name) throw new TypeError("User-defined function name cannot be an empty string");
      const safeIntegers = "safeIntegers" in options ? +getBooleanOption(options, "safeIntegers") : 2;
      const deterministic = getBooleanOption(options, "deterministic");
      const directOnly = getBooleanOption(options, "directOnly");
      const varargs = getBooleanOption(options, "varargs");
      let argCount = -1;
      if (!varargs) {
        argCount = fn.length;
        if (!Number.isInteger(argCount) || argCount < 0) throw new TypeError("Expected function.length to be a positive integer");
        if (argCount > 100) throw new RangeError("User-defined functions cannot have more than 100 arguments");
      }
      this[cppdb].function(fn, name, argCount, safeIntegers, deterministic, directOnly);
      return this;
    };
  }
});

// node_modules/better-sqlite3/lib/methods/aggregate.js
var require_aggregate = __commonJS({
  "node_modules/better-sqlite3/lib/methods/aggregate.js"(exports, module) {
    "use strict";
    var { getBooleanOption, cppdb } = require_util();
    module.exports = function defineAggregate(name, options) {
      if (typeof name !== "string") throw new TypeError("Expected first argument to be a string");
      if (typeof options !== "object" || options === null) throw new TypeError("Expected second argument to be an options object");
      if (!name) throw new TypeError("User-defined function name cannot be an empty string");
      const start = "start" in options ? options.start : null;
      const step = getFunctionOption(options, "step", true);
      const inverse = getFunctionOption(options, "inverse", false);
      const result = getFunctionOption(options, "result", false);
      const safeIntegers = "safeIntegers" in options ? +getBooleanOption(options, "safeIntegers") : 2;
      const deterministic = getBooleanOption(options, "deterministic");
      const directOnly = getBooleanOption(options, "directOnly");
      const varargs = getBooleanOption(options, "varargs");
      let argCount = -1;
      if (!varargs) {
        argCount = Math.max(getLength(step), inverse ? getLength(inverse) : 0);
        if (argCount > 0) argCount -= 1;
        if (argCount > 100) throw new RangeError("User-defined functions cannot have more than 100 arguments");
      }
      this[cppdb].aggregate(start, step, inverse, result, name, argCount, safeIntegers, deterministic, directOnly);
      return this;
    };
    var getFunctionOption = (options, key, required) => {
      const value = key in options ? options[key] : null;
      if (typeof value === "function") return value;
      if (value != null) throw new TypeError(`Expected the "${key}" option to be a function`);
      if (required) throw new TypeError(`Missing required option "${key}"`);
      return null;
    };
    var getLength = ({ length }) => {
      if (Number.isInteger(length) && length >= 0) return length;
      throw new TypeError("Expected function.length to be a positive integer");
    };
  }
});

// node_modules/better-sqlite3/lib/methods/table.js
var require_table = __commonJS({
  "node_modules/better-sqlite3/lib/methods/table.js"(exports, module) {
    "use strict";
    var { cppdb } = require_util();
    module.exports = function defineTable(name, factory) {
      if (typeof name !== "string") throw new TypeError("Expected first argument to be a string");
      if (!name) throw new TypeError("Virtual table module name cannot be an empty string");
      let eponymous = false;
      if (typeof factory === "object" && factory !== null) {
        eponymous = true;
        factory = defer(parseTableDefinition(factory, "used", name));
      } else {
        if (typeof factory !== "function") throw new TypeError("Expected second argument to be a function or a table definition object");
        factory = wrapFactory(factory);
      }
      this[cppdb].table(factory, name, eponymous);
      return this;
    };
    function wrapFactory(factory) {
      return function virtualTableFactory(moduleName, databaseName, tableName, ...args) {
        const thisObject = {
          module: moduleName,
          database: databaseName,
          table: tableName
        };
        const def = apply.call(factory, thisObject, args);
        if (typeof def !== "object" || def === null) {
          throw new TypeError(`Virtual table module "${moduleName}" did not return a table definition object`);
        }
        return parseTableDefinition(def, "returned", moduleName);
      };
    }
    function parseTableDefinition(def, verb, moduleName) {
      if (!hasOwnProperty.call(def, "rows")) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition without a "rows" property`);
      }
      if (!hasOwnProperty.call(def, "columns")) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition without a "columns" property`);
      }
      const rows = def.rows;
      if (typeof rows !== "function" || Object.getPrototypeOf(rows) !== GeneratorFunctionPrototype) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with an invalid "rows" property (should be a generator function)`);
      }
      let columns = def.columns;
      if (!Array.isArray(columns) || !(columns = [...columns]).every((x) => typeof x === "string")) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with an invalid "columns" property (should be an array of strings)`);
      }
      if (columns.length !== new Set(columns).size) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with duplicate column names`);
      }
      if (!columns.length) {
        throw new RangeError(`Virtual table module "${moduleName}" ${verb} a table definition with zero columns`);
      }
      let parameters;
      if (hasOwnProperty.call(def, "parameters")) {
        parameters = def.parameters;
        if (!Array.isArray(parameters) || !(parameters = [...parameters]).every((x) => typeof x === "string")) {
          throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with an invalid "parameters" property (should be an array of strings)`);
        }
      } else {
        parameters = inferParameters(rows);
      }
      if (parameters.length !== new Set(parameters).size) {
        throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with duplicate parameter names`);
      }
      if (parameters.length > 32) {
        throw new RangeError(`Virtual table module "${moduleName}" ${verb} a table definition with more than the maximum number of 32 parameters`);
      }
      for (const parameter of parameters) {
        if (columns.includes(parameter)) {
          throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with column "${parameter}" which was ambiguously defined as both a column and parameter`);
        }
      }
      let safeIntegers = 2;
      if (hasOwnProperty.call(def, "safeIntegers")) {
        const bool = def.safeIntegers;
        if (typeof bool !== "boolean") {
          throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with an invalid "safeIntegers" property (should be a boolean)`);
        }
        safeIntegers = +bool;
      }
      let directOnly = false;
      if (hasOwnProperty.call(def, "directOnly")) {
        directOnly = def.directOnly;
        if (typeof directOnly !== "boolean") {
          throw new TypeError(`Virtual table module "${moduleName}" ${verb} a table definition with an invalid "directOnly" property (should be a boolean)`);
        }
      }
      const columnDefinitions = [
        ...parameters.map(identifier).map((str) => `${str} HIDDEN`),
        ...columns.map(identifier)
      ];
      return [
        `CREATE TABLE x(${columnDefinitions.join(", ")});`,
        wrapGenerator(rows, new Map(columns.map((x, i) => [x, parameters.length + i])), moduleName),
        parameters,
        safeIntegers,
        directOnly
      ];
    }
    function wrapGenerator(generator, columnMap, moduleName) {
      return function* virtualTable(...args) {
        const output = args.map((x) => Buffer.isBuffer(x) ? Buffer.from(x) : x);
        for (let i = 0; i < columnMap.size; ++i) {
          output.push(null);
        }
        for (const row of generator(...args)) {
          if (Array.isArray(row)) {
            extractRowArray(row, output, columnMap.size, moduleName);
            yield output;
          } else if (typeof row === "object" && row !== null) {
            extractRowObject(row, output, columnMap, moduleName);
            yield output;
          } else {
            throw new TypeError(`Virtual table module "${moduleName}" yielded something that isn't a valid row object`);
          }
        }
      };
    }
    function extractRowArray(row, output, columnCount, moduleName) {
      if (row.length !== columnCount) {
        throw new TypeError(`Virtual table module "${moduleName}" yielded a row with an incorrect number of columns`);
      }
      const offset = output.length - columnCount;
      for (let i = 0; i < columnCount; ++i) {
        output[i + offset] = row[i];
      }
    }
    function extractRowObject(row, output, columnMap, moduleName) {
      let count = 0;
      for (const key of Object.keys(row)) {
        const index = columnMap.get(key);
        if (index === void 0) {
          throw new TypeError(`Virtual table module "${moduleName}" yielded a row with an undeclared column "${key}"`);
        }
        output[index] = row[key];
        count += 1;
      }
      if (count !== columnMap.size) {
        throw new TypeError(`Virtual table module "${moduleName}" yielded a row with missing columns`);
      }
    }
    function inferParameters({ length }) {
      if (!Number.isInteger(length) || length < 0) {
        throw new TypeError("Expected function.length to be a positive integer");
      }
      const params = [];
      for (let i = 0; i < length; ++i) {
        params.push(`$${i + 1}`);
      }
      return params;
    }
    var { hasOwnProperty } = Object.prototype;
    var { apply } = Function.prototype;
    var GeneratorFunctionPrototype = Object.getPrototypeOf(function* () {
    });
    var identifier = (str) => `"${str.replace(/"/g, '""')}"`;
    var defer = (x) => () => x;
  }
});

// node_modules/better-sqlite3/lib/methods/inspect.js
var require_inspect = __commonJS({
  "node_modules/better-sqlite3/lib/methods/inspect.js"(exports, module) {
    "use strict";
    var DatabaseInspection = function Database() {
    };
    module.exports = function inspect(depth, opts) {
      return Object.assign(new DatabaseInspection(), this);
    };
  }
});

// node_modules/better-sqlite3/lib/database.js
var require_database = __commonJS({
  "node_modules/better-sqlite3/lib/database.js"(exports, module) {
    "use strict";
    var fs7 = __require("fs");
    var path9 = __require("path");
    var util = require_util();
    var SqliteError = require_sqlite_error();
    var DEFAULT_ADDON;
    function Database(filenameGiven, options) {
      if (new.target == null) {
        return new Database(filenameGiven, options);
      }
      let buffer;
      if (Buffer.isBuffer(filenameGiven)) {
        buffer = filenameGiven;
        filenameGiven = ":memory:";
      }
      if (filenameGiven == null) filenameGiven = "";
      if (options == null) options = {};
      if (typeof filenameGiven !== "string") throw new TypeError("Expected first argument to be a string");
      if (typeof options !== "object") throw new TypeError("Expected second argument to be an options object");
      if ("readOnly" in options) throw new TypeError('Misspelled option "readOnly" should be "readonly"');
      if ("memory" in options) throw new TypeError('Option "memory" was removed in v7.0.0 (use ":memory:" filename instead)');
      const filename = filenameGiven.trim();
      const anonymous = filename === "" || filename === ":memory:";
      const readonly = util.getBooleanOption(options, "readonly");
      const fileMustExist = util.getBooleanOption(options, "fileMustExist");
      const timeout = "timeout" in options ? options.timeout : 5e3;
      const verbose = "verbose" in options ? options.verbose : null;
      const nativeBinding = "nativeBinding" in options ? options.nativeBinding : null;
      if (readonly && anonymous && !buffer) throw new TypeError("In-memory/temporary databases cannot be readonly");
      if (!Number.isInteger(timeout) || timeout < 0) throw new TypeError('Expected the "timeout" option to be a positive integer');
      if (timeout > 2147483647) throw new RangeError('Option "timeout" cannot be greater than 2147483647');
      if (verbose != null && typeof verbose !== "function") throw new TypeError('Expected the "verbose" option to be a function');
      if (nativeBinding != null && typeof nativeBinding !== "string" && typeof nativeBinding !== "object") throw new TypeError('Expected the "nativeBinding" option to be a string or addon object');
      let addon;
      if (nativeBinding == null) {
        addon = DEFAULT_ADDON || (DEFAULT_ADDON = require_bindings()("better_sqlite3.node"));
      } else if (typeof nativeBinding === "string") {
        const requireFunc = typeof __non_webpack_require__ === "function" ? __non_webpack_require__ : __require;
        addon = requireFunc(path9.resolve(nativeBinding).replace(/(\.node)?$/, ".node"));
      } else {
        addon = nativeBinding;
      }
      if (!addon.isInitialized) {
        addon.setErrorConstructor(SqliteError);
        addon.isInitialized = true;
      }
      if (!anonymous && !filename.startsWith("file:") && !fs7.existsSync(path9.dirname(filename))) {
        throw new TypeError("Cannot open database because the directory does not exist");
      }
      Object.defineProperties(this, {
        [util.cppdb]: { value: new addon.Database(filename, filenameGiven, anonymous, readonly, fileMustExist, timeout, verbose || null, buffer || null) },
        ...wrappers.getters
      });
    }
    var wrappers = require_wrappers();
    Database.prototype.prepare = wrappers.prepare;
    Database.prototype.transaction = require_transaction();
    Database.prototype.pragma = require_pragma();
    Database.prototype.backup = require_backup();
    Database.prototype.serialize = require_serialize();
    Database.prototype.function = require_function();
    Database.prototype.aggregate = require_aggregate();
    Database.prototype.table = require_table();
    Database.prototype.loadExtension = wrappers.loadExtension;
    Database.prototype.exec = wrappers.exec;
    Database.prototype.close = wrappers.close;
    Database.prototype.defaultSafeIntegers = wrappers.defaultSafeIntegers;
    Database.prototype.unsafeMode = wrappers.unsafeMode;
    Database.prototype[util.inspect] = require_inspect();
    module.exports = Database;
  }
});

// node_modules/better-sqlite3/lib/index.js
var require_lib = __commonJS({
  "node_modules/better-sqlite3/lib/index.js"(exports, module) {
    "use strict";
    module.exports = require_database();
    module.exports.SqliteError = require_sqlite_error();
  }
});

// cli/entry.ts
import path8 from "path";
import { fileURLToPath as fileURLToPath3 } from "url";

// cli/args.ts
var COMMANDS = /* @__PURE__ */ new Set(["serve", "status", "sessions", "continue", "chat", "bundle", "data", "help"]);
var BUNDLE_SUBCOMMANDS = /* @__PURE__ */ new Set(["pull", "status"]);
var DATA_SUBCOMMANDS = /* @__PURE__ */ new Set(["diagnose", "checkpoints", "restore"]);
var CHANNELS = /* @__PURE__ */ new Set(["stable", "beta"]);
var RUNTIMES = /* @__PURE__ */ new Set(["node", "rust"]);
function parseCliArgs(argv = []) {
  const args = Array.from(argv);
  const command = args[0] && !args[0].startsWith("-") ? args.shift() : "help";
  if (!COMMANDS.has(command)) {
    return { command: "help", error: `unknown command: ${command}` };
  }
  const result = {
    command,
    subcommand: null,
    channel: "stable",
    runtime: "node",
    plain: false,
    url: null,
    token: null,
    session: null,
    target: null,
    allowDataDowngrade: false,
    confirmToken: null,
    passthrough: []
  };
  const usedOptions = /* @__PURE__ */ new Set();
  const markOption = (option) => {
    if (usedOptions.has(option)) throw new Error(`${option} was given more than once`);
    usedOptions.add(option);
  };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--help" || arg === "-h") {
      return { command: "help", error: null };
    } else if (arg === "--plain") {
      markOption(arg);
      result.plain = true;
    } else if (arg === "--allow-data-downgrade") {
      markOption(arg);
      result.allowDataDowngrade = true;
    } else if (arg === "--url") {
      markOption(arg);
      result.url = requireValue(args, ++i, "--url");
    } else if (arg === "--token") {
      markOption(arg);
      result.token = requireValue(args, ++i, "--token");
    } else if (arg === "--session") {
      markOption(arg);
      result.session = requireValue(args, ++i, "--session");
    } else if (arg === "--confirm-token") {
      markOption(arg);
      result.confirmToken = requireValue(args, ++i, "--confirm-token");
    } else if (arg === "--channel") {
      markOption(arg);
      const value = requireValue(args, ++i, "--channel");
      if (!CHANNELS.has(value)) {
        throw new Error(`--channel must be one of: stable, beta (got ${value})`);
      }
      result.channel = value;
    } else if (arg === "--runtime") {
      markOption(arg);
      const value = requireValue(args, ++i, "--runtime");
      if (!RUNTIMES.has(value)) {
        throw new Error(`--runtime must be one of: node, rust (got ${value})`);
      }
      result.runtime = value;
    } else if (arg === "--") {
      if (command !== "serve") {
        return { command: "help", error: `unknown argument: ${arg}` };
      }
      result.passthrough = args.slice(i + 1);
      break;
    } else if (command === "continue" && !result.target && !arg.startsWith("-")) {
      result.target = arg;
    } else if (command === "bundle" && !result.subcommand && !arg.startsWith("-")) {
      result.subcommand = arg;
    } else if (command === "data" && !result.subcommand && !arg.startsWith("-")) {
      result.subcommand = arg;
    } else if (command === "data" && result.subcommand === "restore" && !result.target && !arg.startsWith("-")) {
      result.target = arg;
    } else {
      return { command: "help", error: `unknown argument: ${arg}` };
    }
  }
  if (command === "bundle" && !BUNDLE_SUBCOMMANDS.has(result.subcommand)) {
    return {
      command: "help",
      error: result.subcommand ? `unknown bundle subcommand: ${result.subcommand} (expected pull or status)` : "bundle requires a subcommand: pull or status"
    };
  }
  if (command === "data") {
    if (!DATA_SUBCOMMANDS.has(result.subcommand)) {
      return {
        command: "help",
        error: result.subcommand ? `unknown data subcommand: ${result.subcommand} (expected diagnose, checkpoints, or restore)` : "data requires a subcommand: diagnose, checkpoints, or restore"
      };
    }
    if (result.subcommand === "restore" && !result.target) {
      return { command: "help", error: "data restore requires a transitionId: hana data restore <transitionId>" };
    }
  }
  const allowedOptions = {
    serve: /* @__PURE__ */ new Set(["--runtime", "--channel", "--allow-data-downgrade"]),
    status: /* @__PURE__ */ new Set(["--runtime", "--url", "--token"]),
    sessions: /* @__PURE__ */ new Set(["--runtime", "--url", "--token"]),
    continue: /* @__PURE__ */ new Set(["--runtime", "--url", "--token", "--plain"]),
    chat: /* @__PURE__ */ new Set(["--runtime", "--url", "--token", "--session", "--plain"]),
    bundle: /* @__PURE__ */ new Set(["--channel"]),
    data: new Set(result.subcommand === "restore" ? ["--confirm-token"] : []),
    help: /* @__PURE__ */ new Set()
  }[command];
  for (const option of usedOptions) {
    if (!allowedOptions.has(option)) {
      return { command: "help", error: `${option} is not supported for hana ${command}` };
    }
  }
  if (result.token && !result.url) {
    return { command: "help", error: "--token requires --url" };
  }
  return result;
}
function requireValue(args, index, flag) {
  const value = args[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}
function helpText() {
  return `Hana CLI

Usage:
  hana serve [-- server args]        Start a headless LingxiAgent Server (serves the --channel web frontend, if pulled)
  hana status                       Show local server and agent status
  hana sessions                     List recent sessions
  hana continue [index|path]        Continue a recent session
  hana chat [--plain]               Open chat
  hana bundle pull                  Pull and activate the latest web frontend
  hana bundle status                Show the pulled web frontend status
  hana data diagnose                Read-only data-epoch diagnostics (stamp, journal, checkpoints)
  hana data checkpoints             List available data-epoch recovery checkpoints
  hana data restore <transitionId>  Restore data from a checkpoint (asks for confirmation)

Connection options:
  --runtime <node|rust>            Select the existing Node server or the Rust service (default: node)
  --url <baseUrl>                   Connect to a specific LingxiAgent Server
  --token <token>                   Bearer token for that server
  --session <path>                  Chat in a specific session

Serve options:
  --allow-data-downgrade            Allow this kernel to open a data directory a newer
                                     kernel already touched (risk of silent data corruption)

Channel options:
  --channel <stable|beta>           Release channel for hana serve and hana bundle (default: stable)

Data recovery options:
  --confirm-token <token>           Non-interactive confirmation for \`hana data restore\`.
                                     Must exactly equal "restore <transitionId>". Required
                                     when stdin is not a TTY; there is no way to skip this.
`;
}

// cli/local-server.ts
import fs from "fs";
import path from "path";

// shared/hana-runtime-paths.ts
var import_hana_runtime_paths = __toESM(require_hana_runtime_paths(), 1);
var {
  resolveLingxiHome,
  resolveLingxiPiSdkManagedBinDir,
  resolveLingxiPiSdkResourceLoaderAgentDir,
  resolveLingxiPiSdkResourceLoaderCwd,
  resolveLingxiPiSdkRuntimeRoot
} = import_hana_runtime_paths.default;

// cli/local-server.ts
function resolveCliLingxiHome(env = process.env) {
  const raw = typeof env.LINGXI_HOME === "string" ? env.LINGXI_HOME.trim() : "";
  return resolveLingxiHome(raw || void 0);
}
function readLocalServerInfo({ lingxiHome = resolveCliLingxiHome(), checkProcess = true } = {}) {
  const filePath = path.join(lingxiHome, "server-info.json");
  if (!fs.existsSync(filePath)) {
    return {
      ok: false,
      reason: "missing_server_info",
      filePath,
      message: `No running LingxiAgent Server was found at ${filePath}`
    };
  }
  let info;
  try {
    info = JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (err) {
    return {
      ok: false,
      reason: "invalid_server_info",
      filePath,
      message: `Cannot read ${filePath}: ${err instanceof Error ? err.message : String(err)}`
    };
  }
  if (!Number.isInteger(info?.port) || (info?.port ?? 0) <= 0 || !info?.token) {
    return {
      ok: false,
      reason: "incomplete_server_info",
      filePath,
      message: `${filePath} is missing port or token`
    };
  }
  const pid = info?.pid;
  if (checkProcess && typeof pid === "number" && Number.isInteger(pid) && !isProcessAlive(pid)) {
    return {
      ok: false,
      reason: "stale_server_info",
      filePath,
      message: `LingxiAgent Server process ${info.pid} is no longer running`
    };
  }
  return {
    ok: true,
    filePath,
    info,
    baseUrl: `http://127.0.0.1:${info.port}`,
    token: info.token,
    source: "server-info"
  };
}
function resolveConnection({ url, token, lingxiHome } = {}) {
  if (url) {
    return {
      ok: true,
      baseUrl: stripTrailingSlash(url),
      token: token || "",
      source: "explicit",
      queryTokenAllowed: false
    };
  }
  const local = readLocalServerInfo({ lingxiHome });
  if (!local.ok) return local;
  return {
    ...local,
    baseUrl: stripTrailingSlash(local.baseUrl),
    queryTokenAllowed: true
  };
}
function isProcessAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
function stripTrailingSlash(value) {
  return String(value || "").replace(/\/+$/, "");
}

// cli/client.ts
import WebSocket from "ws";
var LingxiCliClient = class {
  constructor({ baseUrl, token = "", queryTokenAllowed = false }) {
    this.baseUrl = String(baseUrl || "").replace(/\/+$/, "");
    this.token = token;
    this.queryTokenAllowed = queryTokenAllowed;
  }
  async request(path9, opts = {}) {
    const headers = { ...opts.headers || {} };
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    let body = opts.body;
    if (body && typeof body === "object" && !(body instanceof Uint8Array)) {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(body);
    }
    const res = await fetch(`${this.baseUrl}${path9}`, {
      ...opts,
      headers,
      body
    });
    const text = await res.text();
    const data = text ? safeJson(text) : null;
    if (!res.ok) {
      const detail = data?.detail || data?.reason || data?.error || text || res.statusText;
      const err = new Error(`HTTP ${res.status}: ${detail}`);
      err.status = res.status;
      err.data = data;
      throw err;
    }
    return data;
  }
  health() {
    return this.request("/api/health");
  }
  identity() {
    return this.request("/api/server/identity");
  }
  agents() {
    return this.request("/api/agents");
  }
  sessions() {
    return this.request("/api/sessions");
  }
  newSession() {
    return this.request("/api/sessions/new", { method: "POST", body: {} });
  }
  switchSession(sessionPath) {
    return this.request("/api/sessions/switch", {
      method: "POST",
      body: { path: sessionPath }
    });
  }
  createWebSocket() {
    const url = new URL(this.baseUrl.replace(/^http/i, "ws"));
    url.pathname = "/ws";
    url.search = "";
    const headers = {};
    if (this.token && this.queryTokenAllowed) {
      url.searchParams.set("token", this.token);
    } else if (this.token) {
      headers.Authorization = `Bearer ${this.token}`;
    }
    return new WebSocket(url.toString(), { headers });
  }
};
function safeJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

// cli/chat.ts
import readline from "readline";

// shared/yuan-visuals.ts
var FALLBACK_YUAN = "lingxi";
var YUAN_VISUALS = Object.freeze({
  lingxi: Object.freeze({
    yuan: "lingxi",
    symbol: "\u273F",
    moodLabel: "MOOD",
    accent: "#537D96",
    avatar: "Lingxi.png"
  }),
  butter: Object.freeze({
    yuan: "butter",
    symbol: "\u274A",
    moodLabel: "PULSE",
    accent: "#5BA88C",
    avatar: "Butter.png"
  }),
  ming: Object.freeze({
    yuan: "ming",
    symbol: "\u25C8",
    moodLabel: "REFLECT",
    accent: "#8BA4B4",
    avatar: "Ming.png"
  })
});
function normalizeYuan(yuan) {
  const key = String(yuan || "").trim().toLowerCase();
  return Object.prototype.hasOwnProperty.call(YUAN_VISUALS, key) ? key : FALLBACK_YUAN;
}
function getYuanVisual(yuan) {
  return YUAN_VISUALS[normalizeYuan(yuan)];
}
function moodLabelForYuan(yuan) {
  const visual = getYuanVisual(yuan);
  return `${visual.symbol} ${visual.moodLabel}`;
}

// cli/terminal-theme.ts
var ansi = Object.freeze({
  reset: "\x1B[0m",
  bold: "\x1B[1m",
  dim: "\x1B[2m",
  italic: "\x1B[3m",
  red: "\x1B[31m",
  yellow: "\x1B[33m",
  green: "\x1B[32m",
  gray: "\x1B[90m"
});
function color(hex) {
  const value = String(hex || "").replace("#", "");
  if (!/^[0-9a-f]{6}$/i.test(value)) return "";
  const r = parseInt(value.slice(0, 2), 16);
  const g = parseInt(value.slice(2, 4), 16);
  const b = parseInt(value.slice(4, 6), 16);
  return `\x1B[38;2;${r};${g};${b}m`;
}
function createTerminalTheme(yuan) {
  const visual = getYuanVisual(yuan);
  const accent = color(visual.accent);
  return {
    yuan: visual.yuan,
    symbol: visual.symbol,
    moodLabel: moodLabelForYuan(visual.yuan),
    accentColor: visual.accent,
    accent,
    reset: ansi.reset,
    dim: ansi.dim,
    bold: ansi.bold,
    italic: ansi.italic,
    red: ansi.red,
    yellow: ansi.yellow,
    green: ansi.green,
    gray: ansi.gray
  };
}
function paint(theme, text) {
  return `${theme.accent}${text}${ansi.reset}`;
}

// cli/chat.ts
function nonEmptyString(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
var SESSION_STREAM_TYPES = /* @__PURE__ */ new Set([
  "text_delta",
  "mood_start",
  "mood_text",
  "mood_end",
  "thinking_start",
  "thinking_end",
  "tool_start",
  "tool_end",
  "turn_end",
  "status",
  "abort_rejected"
]);
function createCliChatPromptMessage(identity, text) {
  const sessionId = nonEmptyString(identity?.sessionId);
  const sessionPath = nonEmptyString(identity?.sessionPath);
  if (!sessionId || !sessionPath || typeof text !== "string") return null;
  return { type: "prompt", text, sessionId, sessionPath };
}
function createCliChatAbortMessage(identity) {
  const sessionId = nonEmptyString(identity?.sessionId);
  const sessionPath = nonEmptyString(identity?.sessionPath);
  const streamId = nonEmptyString(identity?.streamId);
  if (!sessionId || !sessionPath || !streamId) return null;
  return { type: "abort", sessionId, sessionPath, streamId };
}
function planCliInterrupt(identity) {
  if (identity?.isStreaming !== true && identity?.pendingPrompt !== true) return { kind: "exit" };
  const message = createCliChatAbortMessage(identity);
  if (!message) return { kind: "wait" };
  if (identity?.abortRequestedStreamId === message.streamId) return { kind: "already-requested" };
  return { kind: "abort", message };
}
function cliChatMessageMatchesSession(identity, msg) {
  const sessionId = nonEmptyString(identity?.sessionId);
  const sessionPath = nonEmptyString(identity?.sessionPath);
  const messageSessionId = nonEmptyString(msg?.sessionId);
  const messageSessionPath = nonEmptyString(msg?.sessionPath);
  if (sessionId && messageSessionId && messageSessionId !== sessionId) return false;
  if (sessionPath && messageSessionPath && messageSessionPath !== sessionPath) return false;
  const matchedIdentity = sessionId && messageSessionId === sessionId || sessionPath && messageSessionPath === sessionPath;
  if ((SESSION_STREAM_TYPES.has(msg?.type) || msg?.type === "error" && (messageSessionId || messageSessionPath)) && !matchedIdentity) return false;
  return true;
}
function reduceCliChatStreamIdentity(current, msg) {
  const state = {
    sessionId: nonEmptyString(current?.sessionId),
    sessionPath: nonEmptyString(current?.sessionPath),
    streamId: nonEmptyString(current?.streamId),
    isStreaming: current?.isStreaming === true
  };
  if (!cliChatMessageMatchesSession(state, msg)) return state;
  const messageStreamId = nonEmptyString(msg?.streamId);
  if (msg?.type === "abort_rejected") {
    return messageStreamId ? { ...state, streamId: messageStreamId, isStreaming: true } : state;
  }
  if (msg?.type === "status") {
    if (msg.isStreaming === true && messageStreamId) {
      return { ...state, streamId: messageStreamId, isStreaming: true };
    }
    if (msg.isStreaming === false) {
      if (state.streamId && (!messageStreamId || messageStreamId !== state.streamId)) return state;
      return { ...state, streamId: null, isStreaming: false };
    }
  }
  if (msg?.type === "turn_end") {
    if (state.streamId && (!messageStreamId || messageStreamId !== state.streamId)) return state;
    return { ...state, streamId: null, isStreaming: false };
  }
  if (msg?.type === "error") {
    if (state.streamId && messageStreamId && messageStreamId !== state.streamId) return state;
    return { ...state, streamId: null, isStreaming: false };
  }
  if (messageStreamId) {
    return { ...state, streamId: messageStreamId, isStreaming: true };
  }
  return state;
}
async function printStatus(client, connection) {
  const [health, identity] = await Promise.all([
    client.health(),
    client.identity().catch(() => null)
  ]);
  const theme = createTerminalTheme(health.agentYuan);
  console.log(`${paint(theme, theme.symbol)} LingxiAgent Server`);
  console.log(`  ${ansi.dim}URL${ansi.reset}       ${connection.baseUrl}`);
  console.log(`  ${ansi.dim}Version${ansi.reset}   ${identity?.version || health.version || "unknown"}`);
  console.log(`  ${ansi.dim}Studio${ansi.reset}    ${identity?.studioLabel || identity?.studioId || "local"}`);
  console.log(`  ${ansi.dim}Agent${ansi.reset}     ${health.agent || "Agent"} \xB7 ${theme.yuan} \xB7 ${theme.symbol}`);
  console.log(`  ${ansi.dim}Model${ansi.reset}     ${health.model || "not set"}`);
  console.log(`  ${ansi.dim}Auth${ansi.reset}      ${identity?.credentialKind || "unavailable (identity check failed)"}`);
}
async function printSessions(client, { limit = 20 } = {}) {
  const sessions = await client.sessions();
  if (!sessions.length) {
    console.log(`${ansi.dim}No sessions yet.${ansi.reset}`);
    return [];
  }
  for (const [idx, session] of sessions.slice(0, limit).entries()) {
    console.log(formatSessionLine(session, idx + 1));
  }
  return sessions;
}
async function startChat(client, connection, opts = {}) {
  const ctx = await loadContext(client);
  let theme = createTerminalTheme(ctx.agentYuan);
  let session = await resolveChatSession(client, opts.session || opts.target);
  let sessionPath = session.path;
  let sessionId = session.sessionId || null;
  const ws = client.createWebSocket();
  const plain = opts.plain === true || !process.stdin.isTTY;
  let streaming = false;
  let pendingPrompt = false;
  let abortWhenKnown = false;
  let activeStreamId = null;
  let abortRequestedStreamId = null;
  let currentMood = "";
  let thinkingTimer = null;
  let thinkingFrame = 0;
  let closedIntentionally = false;
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: ""
  });
  function renderHeader() {
    console.log("");
    console.log(`${paint(theme, theme.symbol)} ${ctx.agentName} ${ansi.dim}\xB7 ${theme.yuan} \xB7 ${connection.baseUrl}${ansi.reset}`);
    console.log(`${ansi.dim}Session \xB7 ${session.title || session.firstMessage || session.path}${ansi.reset}`);
    console.log(`${ansi.dim}Type /help for commands.${plain ? " Plain mode is line-oriented." : " Ctrl+C aborts or exits."}${ansi.reset}
`);
  }
  function prompt() {
    process.stdout.write(`${paint(theme, ctx.userName || "you")} ${ansi.dim}\u203A${ansi.reset} `);
  }
  function startThinking() {
    if (thinkingTimer) return;
    const frames = [
      `${theme.symbol} ${ctx.agentName} \u6B63\u5728\u601D\u8003`,
      `${theme.symbol} ${ctx.agentName} \u6B63\u5728\u6574\u7406\u4E0A\u4E0B\u6587`,
      `${theme.symbol} ${ctx.agentName} \u6B63\u5728\u770B\u5DE5\u5177\u8F68\u8FF9`
    ];
    const tick = () => {
      const text = frames[thinkingFrame++ % frames.length];
      process.stdout.write(`\r${ansi.dim}${text}${".".repeat(thinkingFrame % 3 + 1)}${ansi.reset}\x1B[K`);
    };
    tick();
    thinkingTimer = setInterval(tick, 500);
  }
  function stopThinking() {
    if (!thinkingTimer) return;
    clearInterval(thinkingTimer);
    thinkingTimer = null;
    process.stdout.write("\r\x1B[K");
  }
  async function refreshTheme() {
    const next = await loadContext(client).catch(() => null);
    if (!next) return;
    ctx.agentName = next.agentName;
    ctx.userName = next.userName;
    ctx.agentYuan = next.agentYuan;
    theme = createTerminalTheme(ctx.agentYuan);
  }
  async function switchTo(target) {
    session = await resolveChatSession(client, target);
    sessionPath = session.path;
    sessionId = session.sessionId || null;
    activeStreamId = null;
    abortRequestedStreamId = null;
    pendingPrompt = false;
    abortWhenKnown = false;
    streaming = false;
    await refreshTheme();
    console.log(`${ansi.dim}Continued session:${ansi.reset} ${session.title || session.firstMessage || session.path}`);
    prompt();
  }
  async function handleCommand(line) {
    const [cmd, ...parts] = line.slice(1).trim().split(/\s+/);
    if (cmd === "q" || cmd === "quit" || cmd === "exit") {
      closeAndExit(0);
      return;
    }
    if (cmd === "help" || cmd === "h") {
      console.log(`
${paint(theme, "/sessions")}          list recent sessions
${paint(theme, "/continue <n|path>")} continue a session
${paint(theme, "/new")}               create a new session
${paint(theme, "/status")}            show server status
${paint(theme, "/quit")}              exit
`);
      prompt();
      return;
    }
    if (cmd === "sessions") {
      await printSessions(client);
      prompt();
      return;
    }
    if (cmd === "continue") {
      await switchTo(parts.join(" "));
      return;
    }
    if (cmd === "new") {
      const created = await client.newSession();
      session = {
        ...created,
        path: created.path,
        title: null,
        firstMessage: ""
      };
      sessionPath = created.path;
      sessionId = created.sessionId || null;
      activeStreamId = null;
      abortRequestedStreamId = null;
      pendingPrompt = false;
      abortWhenKnown = false;
      streaming = false;
      console.log(`${paint(theme, theme.symbol)} New session`);
      prompt();
      return;
    }
    if (cmd === "status") {
      await printStatus(client, connection);
      prompt();
      return;
    }
    console.log(`${ansi.dim}Unknown command: /${cmd}${ansi.reset}`);
    prompt();
  }
  function closeAndExit(code) {
    closedIntentionally = true;
    try {
      ws.close();
    } catch {
    }
    try {
      rl.close();
    } catch {
    }
    if (process.stdin.isTTY) {
      try {
        process.stdin.setRawMode(false);
      } catch {
      }
    }
    process.exit(code);
  }
  function requestAbort() {
    const plan = planCliInterrupt({
      sessionId,
      sessionPath,
      streamId: activeStreamId,
      isStreaming: streaming,
      pendingPrompt,
      abortRequestedStreamId
    });
    if (plan.kind === "wait") {
      abortWhenKnown = true;
      process.stdout.write(`
${ansi.yellow}Stop queued until the active stream identity is known.${ansi.reset}
`);
      return;
    }
    if (plan.kind === "already-requested") return;
    if (plan.kind === "abort") {
      abortWhenKnown = false;
      ws.send(JSON.stringify(plan.message));
      abortRequestedStreamId = plan.message.streamId;
      process.stdout.write(`
${ansi.dim}Stop requested\u2026${ansi.reset}
`);
    }
  }
  ws.on("open", () => {
    renderHeader();
    prompt();
  });
  ws.on("message", async (data) => {
    const msg = safeParse(data.toString());
    if (!msg) return;
    if (msg.type === "app_event" && (msg.event?.type === "agent-switched" || msg.event?.type === "agent-updated")) {
      await refreshTheme();
      return;
    }
    if (!cliChatMessageMatchesSession({ sessionId, sessionPath }, msg)) return;
    const wasStreaming = streaming;
    const tracked = reduceCliChatStreamIdentity({
      sessionId,
      sessionPath,
      streamId: activeStreamId,
      isStreaming: streaming
    }, msg);
    activeStreamId = tracked.streamId;
    streaming = tracked.isStreaming;
    if (msg.type === "status" && msg.isStreaming === true) pendingPrompt = false;
    if (msg.type === "turn_end" || msg.type === "error" || msg.type === "status" && msg.isStreaming === false) {
      pendingPrompt = false;
      abortWhenKnown = false;
    }
    if (abortWhenKnown && streaming && activeStreamId) requestAbort();
    switch (msg.type) {
      case "text_delta":
        stopThinking();
        if (!wasStreaming) {
          process.stdout.write("\n");
        }
        process.stdout.write(msg.delta || "");
        break;
      case "mood_start":
        currentMood = "";
        break;
      case "mood_text":
        currentMood += msg.delta || "";
        break;
      case "mood_end":
        if (currentMood.trim()) {
          process.stdout.write(`
${theme.accent}${ansi.italic}${theme.moodLabel}${ansi.reset} ${ansi.dim}${currentMood.trim()}${ansi.reset}
`);
        }
        currentMood = "";
        break;
      case "thinking_start":
        startThinking();
        break;
      case "thinking_end":
        stopThinking();
        break;
      case "tool_start":
        stopThinking();
        process.stdout.write(`
${theme.accent}\u25C7${ansi.reset} ${ansi.dim}${msg.name || "tool"}${ansi.reset}`);
        break;
      case "tool_end":
        process.stdout.write(msg.success === false ? ` ${ansi.red}failed${ansi.reset}
` : ` ${ansi.green}done${ansi.reset}
`);
        break;
      case "turn_end":
        if (streaming) return;
        stopThinking();
        abortRequestedStreamId = null;
        process.stdout.write("\n");
        prompt();
        break;
      case "error":
        if (streaming) return;
        stopThinking();
        abortRequestedStreamId = null;
        process.stdout.write(`
${ansi.red}${msg.message || "error"}${ansi.reset}
`);
        prompt();
        break;
      case "status":
        if (!streaming) {
          stopThinking();
          abortRequestedStreamId = null;
        }
        break;
      case "abort_rejected":
        abortRequestedStreamId = null;
        process.stdout.write(`
${ansi.yellow}Stop request ignored because the active stream changed.${ansi.reset}
`);
        break;
      default:
        break;
    }
  });
  ws.on("close", () => {
    stopThinking();
    console.log(`
${ansi.dim}Disconnected.${ansi.reset}`);
    closeAndExit(closedIntentionally ? 0 : 1);
  });
  ws.on("error", (err) => {
    stopThinking();
    console.error(`
${ansi.red}${err.message}${ansi.reset}`);
    closeAndExit(1);
  });
  rl.on("line", async (input) => {
    const line = input.trim();
    if (!line) {
      prompt();
      return;
    }
    if (streaming || pendingPrompt) {
      process.stdout.write(`${ansi.dim}Wait for the current reply or stop it first.${ansi.reset}
`);
      return;
    }
    try {
      if (line.startsWith("/")) {
        await handleCommand(line);
        return;
      }
      const message = createCliChatPromptMessage({ sessionId, sessionPath }, line);
      if (!message) throw new Error("Session identity unavailable; reconnect or choose another session.");
      ws.send(JSON.stringify(message));
      pendingPrompt = true;
    } catch (err) {
      console.log(`${ansi.red}${err.message}${ansi.reset}`);
      prompt();
    }
  });
  readline.emitKeypressEvents(process.stdin, rl);
  if (!plain && process.stdin.isTTY) {
    process.stdin.setRawMode(true);
    process.stdin.on("keypress", (_str, key) => {
      if (!key) return;
      if (key.name === "escape" && (streaming || pendingPrompt)) {
        requestAbort();
      }
      if (key.ctrl && key.name === "c") {
        if (streaming || pendingPrompt) {
          requestAbort();
        } else {
          closeAndExit(0);
        }
      }
    });
  }
}
async function loadContext(client) {
  const [health, agentsResult] = await Promise.all([
    client.health(),
    client.agents().catch(() => ({ agents: [] }))
  ]);
  const agents = Array.isArray(agentsResult.agents) ? agentsResult.agents : [];
  const current = agents.find((agent) => agent.id === health.agentId) || agents.find((agent) => agent.name === health.agent) || agents[0] || null;
  return {
    agentId: health.agentId || current?.id || null,
    agentName: health.agent || current?.name || "Hana",
    agentYuan: health.agentYuan || current?.yuan || "lingxi",
    userName: health.user || "you"
  };
}
async function resolveChatSession(client, target) {
  if (target) {
    const sessions2 = await client.sessions();
    const found = selectSession(sessions2, target);
    if (!found) throw new Error(`Session not found: ${target}`);
    await client.switchSession(found.path);
    return found;
  }
  const sessions = await client.sessions();
  if (sessions[0]) {
    await client.switchSession(sessions[0].path);
    return sessions[0];
  }
  const created = await client.newSession();
  return { ...created, path: created.path, title: null, firstMessage: "" };
}
function selectSession(sessions, target) {
  if (!target) return sessions[0] || null;
  const trimmed = String(target).trim();
  const maybeIndex = Number.parseInt(trimmed, 10);
  if (String(maybeIndex) === trimmed && maybeIndex > 0) {
    return sessions[maybeIndex - 1] || null;
  }
  return sessions.find((session) => session.path === trimmed) || null;
}
function formatSessionLine(session, index) {
  const title = session.title || session.firstMessage || "Untitled";
  const agent = session.agentName || session.agentId || "Agent";
  const modified = session.modified ? new Date(session.modified).toLocaleString() : "";
  return `${ansi.dim}${String(index).padStart(2, " ")}.${ansi.reset} ${title.slice(0, 72)} ${ansi.dim}\xB7 ${agent}${modified ? ` \xB7 ${modified}` : ""}${ansi.reset}`;
}
function safeParse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

// cli/server-runner.ts
import fs2 from "fs";
import path2 from "path";
import { spawn } from "child_process";
import { createRequire } from "module";
var import_server_info_probe = __toESM(require_server_info_probe(), 1);
var require2 = createRequire(import.meta.url);
var activation = require2("../shared/artifact-core/activation.cjs");
var pointerStore = require2("../shared/artifact-core/pointer-store.cjs");
var { rendererPointerChannel } = require2("../shared/artifact-core/pointer-channels.cjs");
async function resolveRendererDistPointer({
  lingxiHome,
  channel = "stable"
}) {
  const rendererChannel = rendererPointerChannel(channel);
  const boot = await activation.resolveBoot(rendererChannel, lingxiHome);
  if (boot) {
    return { distDir: boot.pointer.versionDir, version: boot.pointer.version ?? null, valid: true };
  }
  const current = await pointerStore.readPointer(lingxiHome, rendererChannel, "current");
  if (current && typeof current.versionDir === "string") {
    return { distDir: current.versionDir, version: current.version ?? null, valid: false };
  }
  return null;
}
async function resolveServerSpawnSpec({
  projectRoot,
  env = process.env,
  extraArgs = [],
  channel = "stable"
} = {}) {
  const root = projectRoot || path2.resolve(import.meta.dirname, "..");
  const explicitRoot = env.LINGXI_ROOT && fs2.existsSync(path2.join(env.LINGXI_ROOT, "bootstrap.js")) ? env.LINGXI_ROOT : null;
  const packagedRoot = explicitRoot || (fs2.existsSync(path2.join(root, "bootstrap.js")) && fs2.existsSync(path2.join(root, "bundle", "index.js")) ? root : null);
  const rendererDist = await resolveRendererDistPointer({ lingxiHome: resolveCliLingxiHome(env), channel });
  if (packagedRoot) {
    const spawnEnv2 = {
      ...env,
      LINGXI_ROOT: packagedRoot,
      LINGXI_SERVER_ENTRY: path2.join(packagedRoot, "bundle", "index.js")
    };
    if (rendererDist) spawnEnv2.LINGXI_RENDERER_DIST = rendererDist.distDir;
    return {
      mode: "packaged",
      command: process.execPath,
      args: [path2.join(packagedRoot, "bootstrap.js"), ...extraArgs],
      env: spawnEnv2,
      rendererDist
    };
  }
  const spawnEnv = { ...env };
  if (rendererDist) spawnEnv.LINGXI_RENDERER_DIST = rendererDist.distDir;
  return {
    mode: "source",
    command: process.execPath,
    // server/main-full.ts is the thin closed composition entry: it
    // statically imports server/index.ts's startServer() plus
    // server/composition/full-root.ts's registerClosedRoutes hook and
    // calls one with the other. server/index.ts itself only exports
    // startServer and is not a spawnable entry on its own anymore.
    args: [path2.join(root, "server", "main-full.ts"), ...extraArgs],
    env: spawnEnv,
    rendererDist
  };
}
async function resolveRustServerSpawnSpec({
  projectRoot,
  env = process.env,
  extraArgs = [],
  channel = "stable"
} = {}) {
  const root = projectRoot || path2.resolve(import.meta.dirname, "..");
  const binary = env.LINGXI_SERVICE_BIN || path2.join(root, "rust", "target", "debug", "lingxi-service");
  if (!path2.isAbsolute(binary)) throw new Error("LINGXI_SERVICE_BIN must be an absolute path");
  try {
    fs2.accessSync(binary, fs2.constants.X_OK);
    if (!fs2.statSync(binary).isFile()) throw new Error("not a regular file");
  } catch {
    throw new Error(`Rust service binary is unavailable at ${binary}; build lingxi-service or set LINGXI_SERVICE_BIN`);
  }
  if (env.LINGXI_ALLOW_DATA_DOWNGRADE === "1") {
    throw new Error("Rust service refuses LINGXI_ALLOW_DATA_DOWNGRADE; use an explicit recovery workflow");
  }
  let home = env.LINGXI_HOME;
  for (let i = 0; i < extraArgs.length; i++) {
    if (extraArgs[i] === "--home") home = extraArgs[i + 1];
  }
  if (!home) {
    const configIndex = extraArgs.indexOf("--config");
    if (configIndex >= 0 && extraArgs[configIndex + 1]) {
      try {
        const config = JSON.parse(fs2.readFileSync(extraArgs[configIndex + 1], "utf8"));
        if (config && typeof config.home === "string") home = config.home;
      } catch {
      }
    }
  }
  if (!home && !extraArgs.includes("--test-mode") && !extraArgs.includes("--config")) {
    throw new Error("Rust service needs an explicit --home, LINGXI_HOME, --config, or --test-mode");
  }
  if (channel !== "stable" && (!home || extraArgs.includes("--test-mode"))) {
    throw new Error("Rust service cannot resolve the selected frontend channel without a fixed data home");
  }
  const spawnEnv = { ...env };
  delete spawnEnv.LINGXI_RENDERER_DIST;
  const pointerHome = extraArgs.includes("--test-mode") ? null : home;
  const rendererDist = pointerHome && path2.isAbsolute(pointerHome) ? await resolveRendererDistPointer({ lingxiHome: pointerHome, channel }) : null;
  if (channel !== "stable" && !rendererDist) {
    throw new Error(`No activated ${channel} frontend is available in the selected data home`);
  }
  if (rendererDist) spawnEnv.LINGXI_RENDERER_DIST = rendererDist.distDir;
  return { mode: "rust", command: binary, args: extraArgs, env: spawnEnv, rendererDist, dataHome: pointerHome };
}
async function spawnRustServerForeground({
  projectRoot,
  extraArgs = [],
  env = process.env,
  channel = "stable",
  allowDataDowngrade = false
} = {}) {
  if (allowDataDowngrade) {
    throw new Error("Rust service does not support --allow-data-downgrade; data-version refusal is mandatory");
  }
  const spec = await resolveRustServerSpawnSpec({ projectRoot, env, extraArgs, channel });
  if (spec.dataHome && path2.isAbsolute(spec.dataHome)) {
    const incumbent = readLocalServerInfo({ lingxiHome: spec.dataHome, checkProcess: false });
    if (!incumbent.ok && incumbent.reason !== "missing_server_info") {
      throw new Error(`Cannot rule out an existing Node server in this data home: ${incumbent.message}`);
    }
    if (incumbent.ok) {
      const guard = await guardAgainstForeignServer({ lingxiHome: spec.dataHome });
      if (guard.blocked) throw new Error(guard.message || "A Node server already owns this data home");
    }
  }
  if (spec.rendererDist?.valid) console.log(`serving web frontend ${spec.rendererDist.version}`);
  return new Promise((resolve) => {
    const child = spawn(spec.command, spec.args, { stdio: "inherit", env: spec.env });
    let settled = false;
    const forward = (signal) => {
      if (!settled) child.kill(signal);
    };
    const onInt = () => forward("SIGINT");
    const onTerm = () => forward("SIGTERM");
    process.on("SIGINT", onInt);
    process.on("SIGTERM", onTerm);
    const finish = (code) => {
      if (settled) return;
      settled = true;
      process.off("SIGINT", onInt);
      process.off("SIGTERM", onTerm);
      resolve(code);
    };
    child.once("error", (err) => {
      console.error(`Rust service failed to start: ${err.message}`);
      finish(1);
    });
    child.once("close", (code, signal) => {
      finish(code ?? (signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1));
    });
  });
}
async function guardAgainstForeignServer({
  lingxiHome,
  probeImpl = import_server_info_probe.probeServerInfo
}) {
  const local = readLocalServerInfo({ lingxiHome, checkProcess: false });
  if (!local.ok) return { blocked: false, message: null };
  const probe = await probeImpl({ info: local.info });
  if (!(0, import_server_info_probe.isForeignServerBlocking)(probe.status)) return { blocked: false, message: null };
  return { blocked: true, message: (0, import_server_info_probe.describeForeignServerBlock)({ status: probe.status, info: local.info }) };
}
function buildServeSpawnEnv({
  env,
  allowDataDowngrade,
  warn = (msg) => console.warn(msg)
}) {
  const spawnEnv = { ...env };
  if (allowDataDowngrade) {
    spawnEnv.LINGXI_ALLOW_DATA_DOWNGRADE = "1";
    warn(
      `${ansi.yellow}--allow-data-downgrade: \u5DF2\u663E\u5F0F\u63A5\u53D7\u6570\u636E\u635F\u574F\u98CE\u9669\uFF0C\u65E7\u5185\u6838\u5C06\u653E\u884C\u6253\u5F00\u88AB\u66F4\u9AD8\u6570\u636E epoch \u89E6\u78B0\u8FC7\u7684\u76EE\u5F55\u3002
--allow-data-downgrade: explicitly accepting the data-corruption risk \u2014 this older kernel will be allowed to open a data directory a higher data epoch has touched.${ansi.reset}`
    );
  }
  return spawnEnv;
}
async function spawnServerForeground({
  projectRoot,
  extraArgs = [],
  env = process.env,
  channel = "stable",
  allowDataDowngrade = false,
  probeImpl = import_server_info_probe.probeServerInfo,
  exit = process.exit
} = {}) {
  const guard = await guardAgainstForeignServer({ lingxiHome: resolveCliLingxiHome(env), probeImpl });
  if (guard.blocked) {
    console.error(`${ansi.red}${guard.message}${ansi.reset}`);
    return exit(1);
  }
  const spawnEnv = buildServeSpawnEnv({ env, allowDataDowngrade });
  const spec = await resolveServerSpawnSpec({ projectRoot, env: spawnEnv, extraArgs, channel });
  if (spec.rendererDist && spec.rendererDist.valid) {
    console.log(`serving web frontend ${spec.rendererDist.version}`);
  }
  const child = spawn(spec.command, spec.args, {
    stdio: "inherit",
    env: spec.env
  });
  child.on("exit", (code) => process.exit(code ?? 1));
  return child;
}
async function startLocalServerAndWait({
  projectRoot,
  env = process.env,
  timeoutMs = 3e4,
  intervalMs = 250
} = {}) {
  const lingxiHome = resolveCliLingxiHome(env);
  const existing = readLocalServerInfo({ lingxiHome });
  if (existing.ok) return existing;
  const spec = await resolveServerSpawnSpec({ projectRoot, env, extraArgs: [] });
  const child = spawn(spec.command, spec.args, {
    stdio: "ignore",
    detached: true,
    env: spec.env
  });
  child.unref();
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const info = readLocalServerInfo({ lingxiHome });
    if (info.ok) return { ...info, started: true, serverMode: spec.mode };
    await delay(intervalMs);
  }
  throw new Error(`LingxiAgent Server did not become ready within ${Math.round(timeoutMs / 1e3)}s`);
}
function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// cli/rust-service.ts
import fs3 from "fs";
import path3 from "path";
import { execFileSync } from "child_process";
import { fileURLToPath } from "url";
import crypto from "crypto";
function safeRustTerminalText(value, limit = 256) {
  return String(value ?? "").replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").slice(0, limit);
}
function rustSourceDigest(root) {
  const hash = crypto.createHash("sha256");
  hash.update("rust-toolchain.toml").update("\0");
  hash.update(fs3.readFileSync(path3.join(root, "rust-toolchain.toml"))).update("\0");
  const rustRoot = path3.join(root, "rust");
  const files = [];
  function walk(dir, relative = "") {
    for (const entry of fs3.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      if (!relative && entry.name === "target") continue;
      if (entry.isSymbolicLink()) throw new Error("Rust source symlink refused");
      if (entry.isDirectory()) walk(path3.join(dir, entry.name), name);
      else if (entry.isFile()) files.push(name);
    }
  }
  walk(rustRoot);
  for (const name of files.sort()) {
    hash.update(name).update("\0");
    hash.update(fs3.readFileSync(path3.join(rustRoot, name))).update("\0");
  }
  return hash.digest("hex");
}
function resolveWindowsRustReader(moduleFile = fileURLToPath(import.meta.url), arch = process.arch) {
  const moduleDir = path3.dirname(moduleFile);
  let directory;
  let expectedVersion;
  let sourceRoot = null;
  if (path3.basename(moduleDir) === "cli") {
    sourceRoot = path3.resolve(moduleDir, "..");
    directory = path3.join(sourceRoot, "dist-rust-service", `win-${arch}`);
    expectedVersion = JSON.parse(fs3.readFileSync(path3.join(sourceRoot, "package.json"), "utf8")).version;
  } else {
    if (path3.basename(moduleDir) !== "bundle") throw new Error("unknown Rust CLI installation layout");
    const serverRoot = path3.resolve(moduleDir, "..");
    directory = path3.join(serverRoot, "rust-service");
    if (path3.basename(serverRoot) === "server" && path3.basename(path3.dirname(serverRoot)) === "LingxiCore") {
      expectedVersion = JSON.parse(fs3.readFileSync(path3.join(serverRoot, "package.json"), "utf8")).version;
    } else if (path3.basename(path3.dirname(serverRoot)) === "server" && path3.basename(path3.dirname(path3.dirname(serverRoot))) === "artifacts") {
      const suffix = `-win32-${arch}`;
      if (!path3.basename(serverRoot).endsWith(suffix)) throw new Error("Rust CLI activated version directory mismatch");
      expectedVersion = path3.basename(serverRoot).slice(0, -suffix.length);
      const bundledVersion = JSON.parse(fs3.readFileSync(path3.join(serverRoot, "package.json"), "utf8")).version;
      if (bundledVersion !== expectedVersion) throw new Error("Rust CLI activated package version mismatch");
      const receipt = JSON.parse(fs3.readFileSync(path3.join(serverRoot, ".verified"), "utf8"));
      const home = path3.dirname(path3.dirname(path3.dirname(serverRoot)));
      const pointers = path3.join(home, "artifacts", "pointers");
      if (receipt.version !== expectedVersion || !/^[0-9a-f]{64}$/.test(receipt.sha256 || "")) {
        throw new Error("Rust CLI activated receipt mismatch");
      }
      const found = fs3.readdirSync(pointers).filter((name) => /\.(current|previous)\.json$/.test(name)).some((name) => {
        const pointer = JSON.parse(fs3.readFileSync(path3.join(pointers, name), "utf8"));
        return pointer.kind === "server" && pointer.platformArch === `win32-${arch}` && pointer.version === expectedVersion && pointer.sha256 === receipt.sha256 && path3.resolve(pointer.versionDir || "") === serverRoot;
      });
      if (!found) throw new Error("Rust CLI activated server pointer mismatch");
    } else {
      throw new Error("unknown Rust CLI package layout");
    }
  }
  if (fs3.lstatSync(directory).isSymbolicLink()) throw new Error("Rust reader directory is a link");
  const manifestPath = path3.join(directory, "build.json");
  const manifestStat = fs3.lstatSync(manifestPath);
  if (!manifestStat.isFile() || manifestStat.isSymbolicLink() || manifestStat.size < 1 || manifestStat.size > 4096) {
    throw new Error("invalid Rust reader manifest");
  }
  const manifest = JSON.parse(fs3.readFileSync(manifestPath, "utf8"));
  if (manifest.schemaVersion !== 1 || manifest.platform !== "win" || manifest.arch !== arch || manifest.binary !== "lingxi-service.exe" || !/^[0-9a-f]{64}$/.test(manifest.sha256 || "") || !/^[0-9a-f]{64}$/.test(manifest.contentSha256 || "") || !/^[0-9a-f]{64}$/.test(manifest.sourceSha256 || "") || !/^\d+\.\d+\.\d+$/.test(manifest.toolchain || "") || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(manifest.appVersion || "") || !/^[a-zA-Z0-9_-]+$/.test(manifest.target || "") || manifest.appVersion !== expectedVersion) {
    throw new Error("Rust reader manifest does not match this Windows CLI");
  }
  if (sourceRoot) {
    const toolchain = /^channel\s*=\s*"(\d+\.\d+\.\d+)"/m.exec(
      fs3.readFileSync(path3.join(sourceRoot, "rust-toolchain.toml"), "utf8")
    )?.[1];
    if (manifest.toolchain !== toolchain || manifest.sourceSha256 !== rustSourceDigest(sourceRoot)) {
      throw new Error("Rust reader stage is stale for current CLI source");
    }
  }
  const binary = path3.join(directory, "lingxi-service.exe");
  const stat = fs3.lstatSync(binary);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1024) throw new Error("invalid Rust reader executable");
  const bytes = fs3.readFileSync(binary);
  const pe = bytes.readUInt32LE(60);
  if (bytes.subarray(0, 2).toString() !== "MZ" || bytes.subarray(pe, pe + 4).toString() !== "PE\0\0" || bytes.readUInt16LE(pe + 4) !== (arch === "arm64" ? 43620 : arch === "x64" ? 34404 : 0)) {
    throw new Error("Rust reader is not a matching Windows executable");
  }
  const fullDigest = crypto.createHash("sha256").update(bytes).digest("hex");
  if (fullDigest !== manifest.sha256) throw new Error("Rust reader executable digest mismatch");
  return binary;
}
function readPrivateJson(file, requirePrivate = false) {
  if (process.platform === "win32") {
    const before = fs3.lstatSync(file);
    if (!before.isFile() || before.isSymbolicLink()) throw new Error("unsafe Rust runtime record");
    const runtime = path3.dirname(file);
    const name = path3.basename(file) === "instance.json" ? "instance" : path3.basename(file) === "local-token.json" ? "local-token" : null;
    if (!name || path3.basename(runtime) !== "lingxi-service") {
      throw new Error("unknown Rust runtime record");
    }
    const reader = resolveWindowsRustReader();
    try {
      const bytes = execFileSync(
        reader,
        ["--read-private-runtime-json", path3.dirname(runtime), name],
        {
          encoding: "utf8",
          timeout: 1e4,
          maxBuffer: 65536,
          windowsHide: true,
          stdio: ["ignore", "pipe", "pipe"]
        }
      );
      return JSON.parse(bytes);
    } catch {
      throw new Error("Windows Rust runtime record failed private handle verification or read");
    }
  }
  const flags = fs3.constants.O_RDONLY | (fs3.constants.O_NOFOLLOW || 0);
  const fd = fs3.openSync(file, flags);
  try {
    const stat = fs3.fstatSync(fd);
    if (!stat.isFile() || stat.size > 65536) throw new Error("not a bounded regular file");
    if (requirePrivate && ((stat.mode & 63) !== 0 || typeof process.getuid === "function" && stat.uid !== process.getuid())) {
      throw new Error("local token file is not owner-only");
    }
    return JSON.parse(fs3.readFileSync(fd, "utf8"));
  } finally {
    fs3.closeSync(fd);
  }
}
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
async function readBoundedText(response) {
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks = [];
  let length = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      length += next.value.byteLength;
      if (length > 8 * 1024 * 1024) {
        await reader.cancel();
        throw new Error("Rust service response exceeded 8 MiB");
      }
      chunks.push(Buffer.from(next.value));
    }
    return Buffer.concat(chunks, length).toString("utf8");
  } finally {
    reader.releaseLock();
  }
}
function loopbackUrl(bindAddr, transport) {
  try {
    const url = new URL(`${transport}://${bindAddr}`);
    if (!["127.0.0.1", "[::1]"].includes(url.hostname) || url.username || url.password || url.pathname !== "/" || url.search || url.hash || !Number.isInteger(Number(url.port)) || Number(url.port) < 1) return null;
    return url.origin;
  } catch {
    return null;
  }
}
function readRustLocalService({ lingxiHome }) {
  const runtime = path3.join(lingxiHome, "lingxi-service");
  const recordPath = path3.join(runtime, "instance.json");
  const tokenPath = path3.join(runtime, "local-token.json");
  try {
    fs3.lstatSync(recordPath);
  } catch (err) {
    if (err.code === "ENOENT") {
      return { ok: false, reason: "missing_rust_instance", message: `No Rust service instance record was found at ${recordPath}` };
    }
    return { ok: false, reason: "invalid_rust_instance", message: "Rust service instance record could not be inspected" };
  }
  try {
    const recordRaw = readPrivateJson(recordPath);
    const tokenRaw = readPrivateJson(tokenPath, true);
    if (!isRecord(recordRaw) || !isRecord(tokenRaw)) throw new Error("invalid instance or token record");
    const record = recordRaw;
    const token = tokenRaw;
    const expectedHome = fs3.realpathSync(lingxiHome);
    if (record.serverKind !== "lingxi-service" || record.homePath !== expectedHome || typeof record.instanceId !== "string" || !record.instanceId || token.kind !== "local_token" || token.instanceId !== record.instanceId || typeof token.token !== "string" || !/^[0-9a-f]{32}$/.test(token.token)) {
      throw new Error("instance identity, home, or local token does not match");
    }
    const transport = record.transport === void 0 ? "http" : record.transport;
    if (transport !== "http" && transport !== "https") {
      throw new Error("instance transport is unknown");
    }
    const baseUrl = typeof record.bindAddr === "string" ? loopbackUrl(record.bindAddr, transport) : null;
    if (!baseUrl) throw new Error("instance address is not a loopback service address");
    return { ok: true, backend: "rust", baseUrl, token: token.token, source: "rust-local-token" };
  } catch (err) {
    return {
      ok: false,
      reason: "invalid_rust_instance",
      message: `Cannot use Rust service instance at ${recordPath}: ${err instanceof Error ? err.message : String(err)}`
    };
  }
}
function explicitRustConnection(url, token) {
  try {
    if (typeof token !== "string" || Buffer.byteLength(token, "utf8") > 4096 || /[\u0000-\u001f\u007f]/.test(token)) {
      throw new Error("token contains invalid header characters or exceeds 4096 bytes");
    }
    const parsed = new URL(url);
    if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash || parsed.pathname !== "/") {
      throw new Error("expected an http(s) origin without credentials, path, or query");
    }
    return { ok: true, backend: "rust", baseUrl: parsed.origin, token, source: "explicit" };
  } catch (err) {
    return { ok: false, reason: "invalid_url", message: `Invalid Rust service URL: ${err instanceof Error ? err.message : String(err)}` };
  }
}
var RustCliClient = class {
  baseUrl;
  token;
  source;
  constructor(connection) {
    this.baseUrl = connection.baseUrl;
    this.token = connection.token;
    this.source = connection.source;
  }
  async request(endpoint) {
    const headers = {};
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    let response;
    try {
      response = await fetch(`${this.baseUrl}${endpoint}`, {
        headers,
        redirect: "error",
        signal: AbortSignal.timeout(5e3)
      });
    } catch (err) {
      throw new Error(`Rust service at ${this.baseUrl} is unreachable: ${err instanceof Error ? err.message : String(err)}`);
    }
    const raw = await readBoundedText(response);
    let body;
    try {
      body = raw ? JSON.parse(raw) : null;
    } catch {
      body = null;
    }
    if (!response.ok) {
      const detail = body?.message || body?.reason || body?.error?.message || response.statusText;
      const redacted = this.token ? String(detail).replaceAll(this.token, "[redacted]") : detail;
      throw new Error(`Rust service HTTP ${response.status}: ${safeRustTerminalText(redacted)}`);
    }
    if (!isRecord(body)) throw new Error(`Rust service returned an invalid ${endpoint} response`);
    return body;
  }
  async health() {
    const health = await this.request("/lingxi/v1/health");
    if (health.serverKind !== "lingxi-service" || health.status !== "ok" || typeof health.serverVersion !== "string" || !health.serverVersion.trim()) {
      throw new Error(`The endpoint at ${this.baseUrl} is not a healthy Rust Lingxi service`);
    }
    return health;
  }
  async identity() {
    const identity = await this.request("/lingxi/v1/me");
    if (typeof identity.principalId !== "string" || typeof identity.credentialKind !== "string") {
      throw new Error("Rust service identity response is incomplete");
    }
    if (this.source === "rust-local-token" && (identity.credentialKind !== "loopback_token" || identity.kind !== "local_user")) {
      throw new Error("Rust service did not authenticate the selected local owner token");
    }
    return identity;
  }
  async sessions() {
    if (this.source === "rust-local-token") await this.identity();
    const body = await this.request("/lingxi/v1/sessions");
    if (!Array.isArray(body.sessions) || body.sessions.some((item) => !isRecord(item) || typeof item.sessionId !== "string" || typeof item.title !== "string")) {
      throw new Error("Rust service sessions response is incomplete");
    }
    return body.sessions;
  }
  async session(sessionId) {
    if (!sessionId || /[/?#]/.test(sessionId)) throw new Error("Invalid session ID");
    if (this.source === "rust-local-token") await this.identity();
    return this.request(`/lingxi/v1/sessions/${encodeURIComponent(sessionId)}`);
  }
};

// cli/bundle.ts
import { createRequire as createRequire2 } from "module";
var import_contract_versions = __toESM(require_contract_versions2(), 1);
var require3 = createRequire2(import.meta.url);
var otaCore = require3("../shared/artifact-core/ota-core.cjs");
var pointerStore2 = require3("../shared/artifact-core/pointer-store.cjs");
var { rendererPointerChannel: rendererPointerChannel2 } = require3("../shared/artifact-core/pointer-channels.cjs");
var { loadPinnedKeyset } = require3("../shared/artifact-core/keyset.cjs");
function createProgressRenderer(stream = process.stdout) {
  const isTty = Boolean(stream.isTTY);
  let lastPhase = "";
  let rendered = false;
  const render = (event) => {
    if (isTty) {
      const pct = event.totalBytes > 0 ? Math.min(100, Math.floor(event.receivedBytes / event.totalBytes * 100)) : 0;
      stream.write(`\r\x1B[2K${ansi.dim}${event.phase} renderer${ansi.reset} ${pct}%`);
      rendered = true;
    } else if (event.phase !== lastPhase) {
      stream.write(`${event.phase} renderer...
`);
    }
    lastPhase = event.phase;
  };
  const finish = () => {
    if (isTty && rendered) stream.write("\r\x1B[2K");
  };
  return { render, finish };
}
async function runBundlePull({
  channel = "stable",
  lingxiHome = resolveCliLingxiHome(),
  download = otaCore.downloadAndApplyRendererArtifact
} = {}) {
  const progress = createProgressRenderer();
  const result = await download({
    homeDir: lingxiHome,
    keyset: loadPinnedKeyset(),
    channel,
    serverProtocolVersion: import_contract_versions.SERVER_PROTOCOL_VERSION,
    onProgress: progress.render,
    // Pipeline diagnostics (mirror failover, gate logs) go to stderr so
    // they never corrupt the stdout progress line.
    log: (msg) => console.error(`${ansi.dim}${msg}${ansi.reset}`)
  });
  progress.finish();
  if (result.ok === false) {
    console.error(`${ansi.red}${result.error}${ansi.reset}`);
    return 1;
  }
  if (result.alreadyCurrent) {
    console.log(`Web frontend is already up to date (${result.version}).`);
    return 0;
  }
  console.log(`${ansi.green}Pulled and activated ${result.version}.${ansi.reset} Restart hana serve to take effect.`);
  return 0;
}
async function runBundleStatus({
  channel = "stable",
  lingxiHome = resolveCliLingxiHome()
} = {}) {
  const rendererChannel = rendererPointerChannel2(channel);
  const [current, next, otaState] = await Promise.all([
    pointerStore2.readPointer(lingxiHome, rendererChannel, "current"),
    pointerStore2.readPointer(lingxiHome, rendererChannel, "next"),
    otaCore.readOtaState(lingxiHome)
  ]);
  const state = otaState && otaState[channel] || {};
  if (!current) {
    console.log(`${ansi.dim}No web frontend has been pulled yet. Run:${ansi.reset} hana bundle pull`);
    return 0;
  }
  console.log(`Web frontend ${ansi.dim}(${channel})${ansi.reset}`);
  console.log(`  ${ansi.dim}Version${ansi.reset}   ${current.version || "unknown"}`);
  console.log(`  ${ansi.dim}Train${ansi.reset}     ${Number.isInteger(current.train) ? current.train : "unknown"}`);
  console.log(`  ${ansi.dim}Checked${ansi.reset}   ${typeof state.lastCheckedAt === "string" ? state.lastCheckedAt : "never"}`);
  if (state.available && typeof state.available === "object") {
    const trainSuffix = Number.isInteger(state.available.train) ? ` (train ${state.available.train})` : "";
    console.log(`  ${ansi.dim}Available${ansi.reset} ${state.available.version}${trainSuffix}`);
  }
  if (next) {
    console.log(`  ${ansi.dim}Staged${ansi.reset}    ${next.version || "unknown"} (not yet active)`);
  }
  if (typeof state.lastError === "string" && state.lastError) {
    console.log(`  ${ansi.dim}Last err${ansi.reset}  ${ansi.red}${state.lastError}${ansi.reset}`);
  }
  return 0;
}

// cli/data.ts
import fs6 from "fs";
import path7 from "path";
import readline2 from "readline";

// core/data-epoch-coordinator.ts
var import_data_epoch = __toESM(require_data_epoch(), 1);

// shared/persistence/startup-phases.ts
var STARTUP_PHASES = Object.freeze([
  "desktop_bootstrap",
  "home_guard",
  "epoch_read_preflight",
  "epoch_transition",
  "post_epoch_pre_bind",
  "transport_bind",
  "first_run_seed",
  "identity_seed",
  "engine_construct",
  "engine_init_legacy_migrations",
  "runtime_ready"
]);

// shared/persistence/store-registry.ts
var ALL_SITE_KINDS = [
  "database-open",
  "write-file",
  "append-file",
  "rename",
  "copy-file",
  "mkdir",
  "remove-path",
  "truncate-file",
  "atomic-write",
  "secret-write",
  "persistent-store-constructor"
];
function runtimeSource(module, contract) {
  return { kind: "runtime-contract", module, contract };
}
function directorySource(module, contract) {
  return { kind: "directory-contract", module, contract };
}
function rules(sourceFiles, reason, kinds = ALL_SITE_KINDS, linePattern) {
  return sourceFiles.map((sourceFile) => ({
    sourceFile,
    kinds: [...kinds],
    ...linePattern ? { linePattern } : {},
    reason
  }));
}
function defineStore(input) {
  let schemaContract;
  if (input.schemaSource.kind === "narrow-exemption") {
    schemaContract = {
      kind: "exempt",
      source: input.ownerModule,
      compatibility: input.compatibility ?? "No strict reader contract is claimed by this inventory.",
      reason: input.schemaSource.reason,
      expiresOn: input.schemaSource.expiresOn
    };
  } else {
    schemaContract = {
      kind: input.schemaSource.kind === "directory-contract" ? "protocol" : "runtime-source",
      source: input.schemaSource.kind === "external-versioned" ? `${input.schemaSource.packageName} via ${input.schemaSource.lockfile}` : input.schemaSource.module,
      compatibility: input.compatibility ?? "The named runtime reader is canonical; this inventory does not imply stricter validation than runtime currently performs."
    };
  }
  return Object.freeze({
    id: input.id,
    ownerModule: input.ownerModule,
    pathPattern: input.pathPatterns[0],
    pathPatterns: [...input.pathPatterns],
    pathExclusions: [...input.pathExclusions ?? []],
    pathKind: input.pathKind ?? "file",
    format: input.format,
    schemaSource: input.schemaSource,
    schemaContract,
    openEntry: [...input.openEntry],
    migrationEntry: [...input.migrationEntry ?? []],
    protocolModules: [...input.protocolModules ?? []],
    firstPossibleOpenPhase: input.firstPossibleOpenPhase ?? "engine_construct",
    firstPossibleWritePhase: input.firstPossibleWritePhase ?? "runtime_ready",
    epochPolicy: input.epochPolicy ?? "epoch-managed",
    checkpointPolicy: input.checkpointPolicy ?? "Included in a future epoch checkpoint unless the coordinator explicitly classifies it otherwise.",
    restorePolicy: input.restorePolicy ?? "Restore only through the owning module after reader compatibility is established.",
    affectedByEpochMigration: input.affectedByEpochMigration ?? true,
    bootstrapSafety: input.bootstrapSafety ?? null,
    preCoordinatorReadProjection: input.preCoordinatorReadProjection ?? null,
    identityContract: input.identityContract,
    exemption: input.exemption ?? null,
    siteRules: [...input.siteRules ?? []]
  });
}
var PERSISTENT_STORES = Object.freeze([
  defineStore({
    id: "data-epoch-stamp",
    ownerModule: "shared/data-epoch.cjs",
    pathPatterns: ["data-epoch.json"],
    format: "json",
    schemaSource: runtimeSource("shared/data-epoch.cjs", "DATA_EPOCH stamp parser and atomic writer"),
    openEntry: ["coordinateDataEpochStartup", "inspectDataEpochMaintenance"],
    firstPossibleOpenPhase: "epoch_read_preflight",
    firstPossibleWritePhase: "epoch_transition",
    epochPolicy: "compatible",
    checkpointPolicy: "Never restore this stamp independently of the data tree it describes.",
    restorePolicy: "Recreate only as the final commit record of a coordinated epoch transition.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "epoch-independent",
      reason: "The epoch stamp is coordinator metadata and is never interpreted as an epoch-managed business store.",
      unstampedHomeSafePaths: []
    },
    identityContract: "One high-water stamp belongs to one canonical LINGXI_HOME.",
    siteRules: rules(["shared/data-epoch.cjs"], "Atomically writes the DATA_EPOCH coordinator stamp.", ["atomic-write"], "dataEpochStampPath")
  }),
  defineStore({
    id: "data-epoch-transition-journal",
    ownerModule: "core/data-epoch-coordinator.ts",
    pathPatterns: ["data-epoch-transition.json"],
    format: "json",
    schemaSource: runtimeSource("shared/data-epoch.cjs", "transition journal parser, invariant checks, and durable writer"),
    openEntry: ["coordinateDataEpochStartup", "inspectDataEpochMaintenance"],
    migrationEntry: ["core/data-epoch-migrations.ts DATA_EPOCH_MIGRATIONS"],
    protocolModules: ["core/data-epoch-coordinator.ts", "core/data-epoch-migrations.ts"],
    firstPossibleOpenPhase: "epoch_read_preflight",
    firstPossibleWritePhase: "epoch_transition",
    epochPolicy: "compatible",
    checkpointPolicy: "Never include the transition journal in the affected-store checkpoint it coordinates.",
    restorePolicy: "Never restore independently; maintenance interprets it together with the epoch stamp and verified checkpoint receipt.",
    affectedByEpochMigration: false,
    identityContract: "transitionId identifies one in-progress transition for one canonical LINGXI_HOME.",
    siteRules: [
      ...rules(["shared/data-epoch.cjs"], "Atomically writes the data epoch transition journal.", ["atomic-write"], "dataEpochJournalPath"),
      ...rules(["shared/data-epoch.cjs"], "Removes only a proven committed transition journal tail.", ["remove-path"], "unlink\\(filePath")
    ]
  }),
  defineStore({
    id: "data-epoch-checkpoints",
    ownerModule: "core/data-epoch-checkpoint-provider.ts",
    pathPatterns: ["data-epoch-checkpoints/{transitionId}/**"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: runtimeSource("core/data-epoch-checkpoint-provider.ts", "checkpoint metadata.json layout, per-store capture strategy, and verify byte/sha256 reconciliation"),
    openEntry: ["createDataEpochCheckpointProvider", "pruneDataEpochCheckpoints"],
    firstPossibleOpenPhase: "epoch_transition",
    firstPossibleWritePhase: "epoch_transition",
    epochPolicy: "compatible",
    checkpointPolicy: "This store holds captured checkpoint bytes for other stores; it is never itself listed as an affected store inside the snapshot it produces.",
    restorePolicy: "Read only through the provider's verify contract; a published checkpoint directory is never hand-edited or restored independently of the coordinator's restore path.",
    affectedByEpochMigration: false,
    identityContract: "transitionId identifies one published or in-progress checkpoint directory for one canonical LINGXI_HOME; provider-internal .tmp-/.invalid- suffixes on that name are staging, not separate identities.",
    siteRules: rules(
      ["core/data-epoch-checkpoint-provider.ts"],
      "Captures, verifies, or prunes data epoch transition checkpoints.",
      ["mkdir", "write-file", "rename", "copy-file", "remove-path", "database-open"]
    )
  }),
  defineStore({
    id: "data-epoch-restore-quarantine",
    ownerModule: "core/data-epoch-restore.ts",
    pathPatterns: ["data-epoch-restore-quarantine/{restoreId}/**", "data-epoch-restores.log"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: runtimeSource("core/data-epoch-restore.ts", "restore quarantine layout, restore-receipt.json schema, and append-only data-epoch-restores.log line shape"),
    openEntry: ["restoreDataEpochCheckpoint"],
    firstPossibleOpenPhase: "epoch_transition",
    firstPossibleWritePhase: "epoch_transition",
    epochPolicy: "compatible",
    checkpointPolicy: "Excluded from epoch checkpoints; this store only ever holds the output of an already-completed restore, never an affected store's forward-transition input.",
    restorePolicy: "Never restored; quarantined bytes are historical evidence retained for manual forensic recovery, not replayed automatically by any restore.",
    affectedByEpochMigration: false,
    identityContract: "restoreId identifies one restore transaction's quarantined bytes for one canonical LINGXI_HOME; data-epoch-restores.log is one shared append-only audit trail across every restore for that home.",
    siteRules: rules(
      ["core/data-epoch-restore.ts"],
      "Quarantines pre-restore bytes, copies back checkpointed bytes, and records the restore audit trail.",
      ["mkdir", "rename", "copy-file", "append-file", "atomic-write"]
    )
  }),
  defineStore({
    id: "server-runtime-info",
    ownerModule: "server/index.ts",
    pathPatterns: ["server-info.json"],
    format: "json",
    schemaSource: runtimeSource("server/index.ts", "server-info writer and desktop bootstrap reader"),
    openEntry: ["server/index.ts bootstrap"],
    firstPossibleOpenPhase: "home_guard",
    firstPossibleWritePhase: "home_guard",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; it is live process discovery state.",
    restorePolicy: "Regenerate after transport binding.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "regenerable",
      reason: "Server discovery state is deleted or regenerated from the live bound process and carries no durable user data.",
      unstampedHomeSafePaths: []
    },
    identityContract: "At most one current server discovery record per LINGXI_HOME.",
    siteRules: [
      ...rules(["server/index.ts"], "Writes or deletes live server discovery state.", ["write-file", "remove-path"], "serverInfoPath|server-info[.]json"),
      ...rules(["desktop/main.cjs"], "Deletes stale desktop server discovery state before or after a server lifecycle.", ["remove-path"], "server-info[.]json|serverInfoPath")
    ]
  }),
  defineStore({
    id: "server-node-identity",
    ownerModule: "core/server-identity.ts",
    pathPatterns: ["server-node.json"],
    format: "json",
    schemaSource: runtimeSource("core/server-identity.ts", "server-node identity parser and ensureLocalIdentityRegistries seed"),
    openEntry: ["loadServerIdentity", "ensureLocalIdentityRegistries"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Include with the identity registries for the same LINGXI_HOME.",
    restorePolicy: "Restore before remote transport accepts identity-bound requests.",
    identityContract: "serverNodeId is the stable identity of one LINGXI_HOME server node.",
    siteRules: rules(["core/server-identity.ts"], "Seeds the server-node identity registry.", ["atomic-write"], "serverNodePath")
  }),
  defineStore({
    id: "user-studio-registries",
    ownerModule: "core/server-identity.ts",
    pathPatterns: ["users.json", "studios.json"],
    format: "json",
    schemaSource: runtimeSource("core/server-identity.ts", "user/studio registry readers"),
    openEntry: ["loadServerIdentity", "ensureLocalIdentityRegistries"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Checkpoint users and studios together.",
    restorePolicy: "Restore users/studios before dependent grants or mounts.",
    identityContract: "userId and studioId are durable identities.",
    siteRules: [
      ...rules(["core/server-identity.ts"], "Seeds user or studio identity registries.", ["atomic-write"], "(?:usersPath|studiosPath)"),
      ...rules(["core/local-user-account.ts"], "Updates the local user record in users.json.", ["secret-write"], "USERS_FILE")
    ]
  }),
  defineStore({
    id: "local-user-auth",
    ownerModule: "core/local-user-account.ts",
    pathPatterns: ["local-user-auth.json"],
    format: "json",
    schemaSource: runtimeSource("core/local-user-account.ts", "local authentication secret reader and atomic serializer"),
    openEntry: ["ensureLocalUserAccount", "authenticateLocalUser"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Checkpoint as permission-preserving secret material with its matching users.json identity.",
    restorePolicy: "Restore atomically before local authentication is enabled.",
    identityContract: "The credential belongs to the local userId in the same LINGXI_HOME.",
    siteRules: rules(["core/local-user-account.ts"], "Writes the local user authentication record.", ["secret-write"], "LOCAL_USER_AUTH_FILE")
  }),
  defineStore({
    id: "device-access-registries",
    ownerModule: "core/device-registry.ts",
    pathPatterns: ["devices.json", "device-credentials.json", "pairing-sessions.json"],
    format: "json",
    schemaSource: runtimeSource("core/device-registry.ts", "device, credential, and pairing-session validators/serializers"),
    openEntry: ["ensureDeviceAccessRegistries", "loadDeviceAccessRegistries"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Checkpoint all three registries as one referential set.",
    restorePolicy: "Restore atomically before paired-device authentication is enabled.",
    identityContract: "deviceId, credentialId, and pairingSessionId are durable cross-registry keys.",
    siteRules: rules(["core/device-registry.ts"], "Writes one member of the device-access registry set.")
  }),
  defineStore({
    id: "server-network-config",
    ownerModule: "core/server-network-config.ts",
    pathPatterns: ["server-network.json"],
    format: "json",
    schemaSource: runtimeSource("core/server-network-config.ts", "server network mode/listen configuration validator"),
    openEntry: ["ensureServerNetworkConfig", "loadServerNetworkConfig"],
    firstPossibleOpenPhase: "post_epoch_pre_bind",
    firstPossibleWritePhase: "post_epoch_pre_bind",
    checkpointPolicy: "Include as operator configuration.",
    restorePolicy: "Validate before binding; invalid restored values fail explicitly.",
    identityContract: "One network configuration belongs to one LINGXI_HOME server.",
    siteRules: rules(["core/server-network-config.ts"], "Writes validated server network configuration.")
  }),
  defineStore({
    id: "studio-mount-registry",
    ownerModule: "core/studio-mounts.ts",
    pathPatterns: ["studio-mounts.json"],
    format: "json",
    schemaSource: runtimeSource("core/studio-mounts.ts", "schemaVersion 1 studio mount registry validator"),
    openEntry: ["ensureStudioMountRegistry", "loadStudioMountRegistry"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Include mount declarations but never copy external mounted content implicitly.",
    restorePolicy: "Restore declarations, then revalidate each external root before granting access.",
    identityContract: "mountId is durable; external paths are locators that require revalidation.",
    siteRules: rules(["core/studio-mounts.ts"], "Writes the studio mount registry.")
  }),
  defineStore({
    id: "web-session-registry",
    ownerModule: "core/web-session-store.ts",
    pathPatterns: ["web-sessions.json"],
    format: "json",
    schemaSource: runtimeSource("core/web-session-store.ts", "web session registry validator, expiry, and serializer"),
    openEntry: ["ensureWebSessionRegistry", "WebSessionStore"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Exclude expired sessions; preserve only live sessions when auth continuity is required.",
    restorePolicy: "Load through WebSessionStore so expiry and validation run before use.",
    identityContract: "webSessionId is durable only for the validated lifetime of its user-bound session.",
    siteRules: rules(["core/web-session-store.ts"], "Writes the web session registry.")
  }),
  defineStore({
    id: "security-grants",
    ownerModule: "core/grant-registry.ts",
    pathPatterns: ["security/grants.json", "security/grants.json.corrupt-{timestamp}", "security/grants.json.corrupt-{timestamp}.reason.txt"],
    format: "json",
    schemaSource: runtimeSource("core/grant-registry.ts", "grant registry validator, quarantine, and atomic serializer"),
    openEntry: ["ensureGrantRegistry", "loadGrantRegistry"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Checkpoint the validated registry; quarantine evidence is diagnostic and optional.",
    restorePolicy: "Validate before enabling grants; corrupt input is quarantined and fails visibly.",
    identityContract: "grantId and its principal/resource scope are the durable authorization identity.",
    siteRules: rules(["core/grant-registry.ts"], "Writes, repairs, or quarantines the grant registry.")
  }),
  defineStore({
    id: "execution-leases",
    ownerModule: "core/execution-lease-registry.ts",
    pathPatterns: ["security/execution-leases.json"],
    format: "json",
    schemaSource: runtimeSource("core/execution-lease-registry.ts", "execution lease registry validator and atomic serializer"),
    openEntry: ["ensureExecutionLeaseRegistry", "loadExecutionLeaseRegistry"],
    firstPossibleOpenPhase: "identity_seed",
    firstPossibleWritePhase: "identity_seed",
    checkpointPolicy: "Exclude expired leases and preserve live leases only with their grant principals.",
    restorePolicy: "Reload through the lease validator and re-evaluate expiry before execution resumes.",
    identityContract: "leaseId is durable only within its principal, grant, and expiry scope.",
    siteRules: rules(["core/execution-lease-registry.ts"], "Writes the execution lease registry.")
  }),
  defineStore({
    id: "security-key-material",
    ownerModule: "core/resource-ticket-service.ts",
    pathPatterns: ["security/resource-ticket-key"],
    format: "mixed-directory",
    schemaSource: directorySource("core/resource-ticket-service.ts", "independent 0600 random key-file protocol for the resource ticket service"),
    openEntry: ["security ticket service construction"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    checkpointPolicy: "Checkpoint exact bytes with restrictive permissions when continuity of issued tickets is required.",
    restorePolicy: "Restore before constructing the matching service; missing keys rotate and invalidate old tickets.",
    identityContract: "Each fixed filename is a distinct service signing boundary; key bytes must never be interchanged.",
    siteRules: rules(["core/resource-ticket-service.ts"], "Creates one service-specific signing key with mode 0600.")
  }),
  defineStore({
    id: "security-audit-log",
    ownerModule: "core/security-audit-log.ts",
    pathPatterns: ["logs/security-audit.jsonl"],
    format: "append-only-log",
    schemaSource: runtimeSource("core/security-audit-log.ts", "one JSON object per append-only security audit line"),
    openEntry: ["appendSecurityAuditRecord"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    checkpointPolicy: "Preserve byte order and complete lines; never merge by object key.",
    restorePolicy: "Append only after the restored final newline; malformed tails require explicit repair.",
    identityContract: "Log sequence and record timestamps provide provenance; the path is a fixed locator.",
    siteRules: rules(["core/security-audit-log.ts"], "Appends a security audit record.")
  }),
  defineStore({
    id: "user-preferences",
    ownerModule: "core/preferences-manager.ts",
    pathPatterns: ["user/preferences.json", "user/preferences.json.corrupt-{timestamp}"],
    format: "json",
    schemaSource: runtimeSource("core/preferences-manager.ts", "PreferencesManager defaults, coercion, and _dataVersion migration driver"),
    openEntry: ["new PreferencesManager"],
    migrationEntry: ["core/preferences-manager.ts unreadable-source preservation", "core/migrations.ts runMigrations"],
    firstPossibleOpenPhase: "desktop_bootstrap",
    firstPossibleWritePhase: "first_run_seed",
    compatibility: "PreferencesManager is intentionally permissive; _dataVersion is the legacy migration cursor, not a strict schema validator. Unreadable source bytes are preserved before any replacement document is written.",
    identityContract: "One preferences document belongs to the local user in one LINGXI_HOME.",
    preCoordinatorReadProjection: {
      compatibility: "additive-only",
      fields: [
        "auto_check_updates",
        "hardware_acceleration",
        "keep_awake",
        "locale",
        "network_proxy",
        "primaryAgent",
        "quick_chat",
        "setupComplete",
        "update_channel"
      ],
      reason: "Desktop startup reads only these named optional fields before the epoch transition; removal or semantic reinterpretation requires moving the read behind the coordinator."
    },
    siteRules: [
      ...rules(["core/preferences-manager.ts"], "Reads and atomically writes user preferences."),
      ...rules(["core/engine.ts"], "Constructs the preferences owner.", ["persistent-store-constructor"], "PreferencesManager")
    ]
  }),
  defineStore({
    id: "provider-state",
    ownerModule: "core/provider-catalog.ts",
    pathPatterns: [
      "provider-catalog.json",
      "added-models.yaml",
      "models.json",
      "auth.json",
      "provider-plugins/{storageId}/manifest.json",
      "provider-plugins/{storageId}/providers/{storageId}.json"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("core/provider-catalog.ts", "ProviderCatalogStore plus provider plugin/config runtime readers"),
    openEntry: ["new ProviderRegistry", "new ProviderCatalogStore"],
    migrationEntry: ["core/provider-auth-migration.ts"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    identityContract: "providerId is the durable key; YAML/JSON paths are storage locators. Locally defined provider plugins live under a filesystem-safe storage identifier derived from that key, which is why their patterns say storageId rather than providerId.",
    siteRules: rules([
      "core/credential-backup-retention.ts",
      "core/local-provider-plugin-store.ts",
      "core/model-sync.ts",
      "core/provider-auth-migration.ts",
      "core/provider-catalog.ts",
      "server/routes/providers.ts"
    ], "Owns provider catalog, authentication, model compatibility, local provider plugin state, or the retention of their migration backups.")
  }),
  defineStore({
    id: "agent-profile",
    ownerModule: "core/agent-manager.ts",
    pathPatterns: [
      "agents/{agentId}/config.yaml",
      "agents/{agentId}/identity.md",
      "agents/{agentId}/AGENTS.md",
      "agents/{agentId}/AGENTS.public.md",
      "agents/{agentId}/appearance-summary.json",
      "agents/{agentId}/avatars/{fileName}",
      "agents/{agentId}/channels.md",
      "user/avatars/{fileName}"
    ],
    pathKind: "file",
    format: "mixed-directory",
    schemaSource: directorySource("core/agent-manager.ts", "agent config and authored Markdown/image profile protocol"),
    openEntry: ["AgentManager.loadAgents", "new LingxiAgent"],
    migrationEntry: ["lib/compat/checks/config-yaml.ts", "core/agents-md-migration.ts"],
    firstPossibleOpenPhase: "first_run_seed",
    firstPossibleWritePhase: "first_run_seed",
    identityContract: "agentId owns the profile; each filename has a fixed semantic role.",
    siteRules: [
      ...rules(["core/agent-manager.ts"], "Creates, rolls back, or edits agent profile material.", ["write-file", "copy-file", "mkdir", "remove-path"]),
      // Startup pass that moves each agent's persona file from its former name
      // onto AGENTS.md / AGENTS.public.md. It only ever renames within one
      // agent directory, which is why "rename" is the single kind it may use.
      ...rules(["core/agents-md-migration.ts"], "Renames agent persona files onto their current names.", ["rename"]),
      // server/routes/config.ts removed from this rule: its only fs write sites were the bare
      // GET/PUT /api/identity and persona-file handlers, deleted as dead legacy routes; the file
      // now only reads agent profile material directly (writes for /pinned and /user-profile go
      // through library helpers, not literal fs calls in this file).
      ...rules(["lib/agent-appearance-summary.ts", "lib/compat/checks/config-yaml.ts", "server/routes/agents.ts", "server/routes/avatar.ts"], "Reads, repairs, removes, or edits agent/user profile material.", ["write-file", "copy-file", "rename", "mkdir", "remove-path", "atomic-write"])
    ]
  }),
  defineStore({
    id: "agent-facts-sqlite",
    ownerModule: "lib/memory/fact-store.ts",
    pathPatterns: ["agents/{agentId}/memory/facts.db"],
    format: "sqlite",
    schemaSource: { kind: "sqlite-runtime", module: "lib/memory/fact-store.ts", contract: "FactStore runtime DDL, PRAGMA user_version, and introspection" },
    openEntry: ["new FactStore"],
    migrationEntry: ["FactStore store-local migrations", "lib/compat/checks/facts-db.ts"],
    compatibility: "The FactStore runtime and its introspection are canonical; test-only v1 DDL is migration fixture data, never the schema source.",
    identityContract: "One facts database belongs to one agentId; SQLite user_version is store-local and never DATA_EPOCH.",
    siteRules: [
      ...rules(["lib/memory/fact-store.ts", "lib/compat/checks/facts-db.ts", "hub/channel-router.ts"], "Opens or repairs the canonical per-agent facts database."),
      ...rules(["core/agent.ts", "lib/character-cards/service.ts", "server/routes/config.ts"], "Constructs or migrates a FactStore.", ["database-open", "persistent-store-constructor"], "(?:FactStore|better-sqlite3|new Database)")
    ]
  }),
  defineStore({
    id: "agent-memory",
    ownerModule: "lib/memory/compile.ts",
    pathPatterns: [
      "agents/{agentId}/memory/memory.md",
      "agents/{agentId}/memory/navigation.md",
      "agents/{agentId}/memory/tenets.json",
      "agents/{agentId}/memory/pinned-tenets-migration.receipt.json",
      "agents/{agentId}/memory/pinned-recovery-operations/{operationId}.json",
      "agents/{agentId}/memory/pinned-migration-backup/{backupFile}",
      "agents/{agentId}/memory/pinned-migration-backup/{operationId}/{backupFile}",
      "agents/{agentId}/memory/facts.md",
      "agents/{agentId}/memory/today.md",
      "agents/{agentId}/memory/week.md",
      "agents/{agentId}/memory/longterm.md",
      "agents/{agentId}/memory/editable-facts-state.json",
      "agents/{agentId}/memory/today-state.json",
      "agents/{agentId}/memory/daily-state.json",
      "agents/{agentId}/memory/summaries/{sessionId}.json",
      "agents/{agentId}/memory/daily/{date}.md",
      "agents/{agentId}/memory/dream/state.json",
      "agents/{agentId}/memory/dream/pending-apply.json",
      "agents/{agentId}/memory/dream/revisions/{revisionId}.json",
      "agents/{agentId}/memory/memories.db",
      "agents/{agentId}/memory/.v2-migrated"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("lib/memory/compiled-memory-state.ts", "compiled memory, rolling-summary, tenets, and daily-state runtime readers"),
    openEntry: ["LingxiAgent.init", "memory ticker and summary stores"],
    migrationEntry: ["LingxiAgent v1 memories.db import"],
    identityContract: "agentId owns long-term memory; session summaries are keyed by sessionId, never by sessionPath.",
    siteRules: [
      ...rules(["core/agent.ts"], "Creates the memory directory and records legacy memories.db import state.", ["write-file", "mkdir"]),
      // Startup one-shot: merges legacy pinned.md / pinned-memory.json items into
      // tenets.json (through the tenets library), keeps a content-addressed local
      // backup, maintains the migration receipt state machine, and renames the old
      // files to *.migrated within the same agent directory.
      ...rules(["core/pinned-tenets-migration.ts"], "\u5C06\u65E7 pins \u5408\u5E76\u5230 tenets\uFF0C\u5199 v3 \u6536\u636E\u4E0E\u539F\u5B57\u8282\u5907\u4EFD\uFF1B\u72EC\u5360\u590D\u5236\u5E76\u6821\u9A8C\u5F52\u6863\u540E\u79FB\u9664\u539F\u540D\u3002", ["write-file", "rename", "mkdir", "copy-file", "remove-path", "atomic-write"]),
      // Explicit, approval-gated recovery of archived .migrated pinned sources;
      // same batch import + receipt mechanism as the startup migration. Never
      // wired into any startup path.
      ...rules(["core/pinned-tenets-recovery.ts"], "Restores approved archived pinned items and writes the recovery receipt.", ["write-file", "mkdir", "atomic-write"]),
      ...rules([
        "lib/memory/cache-snapshot-observation.ts",
        "lib/memory/compile.ts",
        "lib/memory/compiled-memory-snapshot.ts",
        "lib/memory/compiled-memory-state.ts",
        "lib/memory/config-loader.ts",
        "lib/memory/dream/revision-store.ts",
        "lib/memory/dream/state-store.ts",
        "lib/memory/memory-ticker.ts",
        "lib/memory/navigation.ts",
        "lib/memory/session-summary.ts",
        "lib/memory/tenets.ts"
      ], "Writes a named per-agent memory or Dream revision protocol file.")
    ]
  }),
  defineStore({
    id: "file-history-sqlite",
    ownerModule: "lib/file-history/history-store.ts",
    pathPatterns: [
      "file-history/{workspaceHash}/history.sqlite",
      "file-history/{workspaceHash}/history.sqlite-wal",
      "file-history/{workspaceHash}/history.sqlite-shm"
    ],
    format: "sqlite",
    schemaSource: { kind: "sqlite-runtime", module: "lib/file-history/history-store.ts", contract: "FileHistoryStore runtime DDL and meta.schema_version" },
    openEntry: ["new FileHistoryStore"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    identityContract: "workspaceHash derives from the resolved workspace root path; one database per workspace; rel_path inside is workspace-relative posix.",
    siteRules: [
      ...rules(["lib/file-history/history-store.ts"], "Opens and writes the per-workspace file-history database."),
      ...rules(["lib/file-history/file-history-service.ts"], "Constructs per-workspace stores and manages their lifecycle.", ["persistent-store-constructor"], "FileHistoryStore")
    ]
  }),
  defineStore({
    id: "session-manifest-sqlite",
    ownerModule: "core/session-manifest/store.ts",
    pathPatterns: ["session-manifest.db", "session-manifest.db-wal", "session-manifest.db-shm"],
    format: "sqlite",
    schemaSource: { kind: "sqlite-runtime", module: "core/session-manifest/store.ts", contract: "SessionManifestStore runtime DDL and store-local PRAGMA user_version" },
    openEntry: ["new SessionManifestStore"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    identityContract: "sessionId is the durable identity; sessionPath is only the current JSONL locator.",
    siteRules: [
      ...rules(["core/session-manifest/store.ts", "core/session-manifest/db-files.ts"], "Opens or manages files for session-manifest.db."),
      ...rules(["core/engine.ts"], "Constructs the manifest store during engine construction.", ["persistent-store-constructor"], "SessionManifestStore")
    ]
  }),
  defineStore({
    id: "session-jsonl",
    ownerModule: "core/session-coordinator.ts",
    pathPatterns: [
      "agents/{agentId}/sessions/{sessionId}.jsonl",
      "agents/{agentId}/sessions/archived/{sessionId}.jsonl",
      "agents/{agentId}/activity/{sessionId}.jsonl",
      "agents/{agentId}/subagent-sessions/{sessionId}.jsonl",
      "agents/{agentId}/.ephemeral/{sessionId}.jsonl"
    ],
    format: "jsonl",
    schemaSource: {
      kind: "external-versioned",
      packageName: "@earendil-works/pi-coding-agent",
      lockfile: "package-lock.json integrity",
      versionSource: "Pi CURRENT_SESSION_VERSION",
      extensions: ["core/session-jsonl-file.ts repair contract", "core/session-coordinator.ts Hana metadata extensions"]
    },
    openEntry: ["SessionCoordinator create/restore", "Pi SessionManager"],
    migrationEntry: ["core/session-jsonl-file.ts"],
    compatibility: "The locked Pi package schema/version plus Hana repair and extension readers are canonical; Hana does not duplicate Pi's session schema.",
    identityContract: "sessionId is identity. sessionPath is a mutable locator and must be resolved at boundaries before keyed state access.",
    siteRules: [
      ...rules([
        "core/session-coordinator.ts",
        "core/session-jsonl-file.ts",
        "core/slash-commands/session-ops.ts",
        "hub/agent-executor.ts"
      ], "Creates, repairs, archives, restores, or appends session JSONL locators."),
      ...rules(["server/routes/sessions.ts"], "Archives, restores, or deletes a session JSONL locator.", ["mkdir", "rename", "remove-path"]),
      ...rules(["core/engine.ts"], "Deletes an expired legacy ephemeral session JSONL locator.", ["remove-path"], "filePath")
    ]
  }),
  defineStore({
    id: "session-sidecars",
    ownerModule: "core/bridge-session-manager.ts",
    pathPatterns: [
      "agents/{agentId}/sessions/session-meta.json",
      "agents/{agentId}/sessions/session-titles.json",
      "agents/{agentId}/sessions/bridge/bridge-sessions.json",
      "agents/{agentId}/sessions/session-vision-notes.json",
      "agents/{agentId}/subagent-sessions/session-meta.json"
    ],
    format: "json",
    schemaSource: runtimeSource("core/bridge-session-manager.ts", "bridge index, legacy session metadata, title, and vision sidecar readers"),
    openEntry: ["SessionCoordinator", "BridgeSessionManager", "VisionBridge"],
    identityContract: "Entries are keyed by sessionId where available; any sessionPath field is only a locator compatibility field.",
    siteRules: rules(["core/bridge-session-manager.ts", "core/vision-bridge.ts"], "Writes a session-owned sidecar or bridge index.")
  }),
  defineStore({
    id: "workspace-snapshots",
    ownerModule: "core/workspace-snapshots.ts",
    pathPatterns: [
      "workspace-snapshots/{workspaceHash}/**",
      "agents/{agentId}/sessions/{sessionId}.jsonl.snapshots.json",
      "agents/{agentId}/sessions/archived/{sessionId}.jsonl.snapshots.json",
      "agents/{agentId}/activity/{sessionId}.jsonl.snapshots.json",
      "agents/{agentId}/subagent-sessions/{sessionId}.jsonl.snapshots.json",
      "agents/{agentId}/.ephemeral/{sessionId}.jsonl.snapshots.json"
    ],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("core/workspace-snapshots.ts", "shadow git repository layout (git init plus info/exclude allowlist, append-only commits) and the {sessionPath}.snapshots.json sidecar schema (version, workspaceRoot, snapshots[])"),
    openEntry: ["WorkspaceSnapshotService"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded from epoch checkpoints; shadow repos are re-capturable workspace-derived caches, and a sidecar without its repo deliberately degrades to write/edit backup fallback during rollback.",
    restorePolicy: "Never restored from epoch checkpoints; rollback reads shadow commits read-only via --git-dir and writes user files back through ResourceIO, which records its own file history.",
    affectedByEpochMigration: false,
    identityContract: "workspaceHash (sha256 prefix of the resolved workspace root) identifies one shadow repo; the sidecar is keyed by its adjacent session JSONL locator and binds turnInputEntryId to shadow commits; sessionId remains the durable identity.",
    siteRules: rules(
      ["core/workspace-snapshots.ts"],
      "Creates the shadow snapshot repository and writes per-session snapshot sidecars."
    )
  }),
  defineStore({
    id: "session-checkpoints",
    ownerModule: "core/session-checkpoints.ts",
    pathPatterns: [
      "agents/{agentId}/sessions/{sessionId}.jsonl.checkpoints.json",
      "agents/{agentId}/sessions/archived/{sessionId}.jsonl.checkpoints.json"
    ],
    format: "json",
    schemaSource: runtimeSource("core/session-checkpoints.ts", "Session checkpoint sidecar schema (schemaVersion, records[{name,target,turnInputEntryId,snapshotCommit,snapshotDegraded,createdAt,messageCount}]), 'latest' rolling slot, name uniqueness, lazy record cap"),
    openEntry: ["listSessionCheckpoints", "upsertSessionCheckpoint"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded from epoch checkpoints; checkpoints are advisory rewind anchors referencing branch entryIds \u2014 losing the sidecar only loses the anchors, never conversation data (the session JSONL is the durable record).",
    restorePolicy: "Never restored from epoch checkpoints; a lost sidecar simply has no rewind anchors until new checkpoints are created.",
    affectedByEpochMigration: false,
    identityContract: "Records are keyed by checkpoint name within the sidecar adjacent to the owning session JSONL; sessionId remains the durable identity.",
    siteRules: rules(
      ["core/session-checkpoints.ts"],
      "Writes per-session checkpoint sidecars (atomic tmp+rename)."
    )
  }),
  defineStore({
    id: "ephemeral-scanner-scratch",
    ownerModule: "lib/security/external-scanners.ts",
    // os.tmpdir() 下的即用即删草稿（gitleaks JSON 报告 / 解包残料），不在
    // lingxiHome 内、进程退出即失效——登记只为让持久化扫描器能归类这些
    // 短命写删点，不表示任何跨进程状态。
    pathPatterns: ["(os-tmpdir)/lingxi-*"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: runtimeSource("lib/security/external-scanners.ts", "mkdtemp scratch dirs consumed within one call and removed in finally; no cross-call state"),
    openEntry: ["runGitleaks / runSemgrep"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; deleted in the same call that created it.",
    restorePolicy: "Never restored; nothing survives the call.",
    affectedByEpochMigration: false,
    identityContract: "No identity \u2014 per-call scratch keyed by mkdtemp random suffix.",
    siteRules: rules(
      ["lib/security/external-scanners.ts", "lib/tools/run-code-tool.ts"],
      "Creates and removes per-call scratch files/directories under the OS temp dir (ast-grep-binary's own scratch sites are owned by managed-runtime-caches)."
    )
  }),
  defineStore({
    id: "session-goal",
    ownerModule: "lib/goal/goal-engine.ts",
    pathPatterns: [
      "agents/{agentId}/sessions/{sessionId}.jsonl.goal.json",
      "agents/{agentId}/sessions/archived/{sessionId}.jsonl.goal.json"
    ],
    format: "json",
    schemaSource: runtimeSource("lib/goal/goal-engine.ts", "Per-session budget goal sidecar schema (schemaVersion, name, tokenBudget, timeBudgetMs, tokensUsed, startedAt, pausedMs, currentPausedAt, status, pausedReason, completedAt, overBudgetNotified, lastActiveAt); one goal per session, active/paused/completed/dropped state machine"),
    openEntry: ["createGoalEngine + goal tool"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded from epoch checkpoints; a budget goal is an advisory session ledger \u2014 losing it loses the budget tracking, never conversation data.",
    restorePolicy: "Never restored from epoch checkpoints; a lost sidecar simply has no active goal.",
    affectedByEpochMigration: false,
    identityContract: "One goal sidecar belongs to the session JSONL it is adjacent to; sessionId remains the durable identity.",
    siteRules: rules(
      ["lib/goal/goal-engine.ts"],
      "Writes the per-session goal sidecar (atomic tmp+rename)."
    )
  }),
  defineStore({
    id: "session-context-notes",
    ownerModule: "lib/tools/context-notes-tool.ts",
    pathPatterns: [
      "agents/{agentId}/sessions/{sessionId}.jsonl.context-notes.md",
      "agents/{agentId}/sessions/archived/{sessionId}.jsonl.context-notes.md"
    ],
    format: "markdown",
    schemaSource: runtimeSource("lib/tools/context-notes-tool.ts", "Per-session markdown notes sidecar, 16KiB write cap with read-side defensive truncation; read/write/append semantics"),
    openEntry: ["context_notes tool"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded from epoch checkpoints; notes are model-authored scratch context re-injected at compaction time \u2014 losing them loses the notes, never conversation data.",
    restorePolicy: "Never restored from epoch checkpoints; a lost sidecar starts empty.",
    affectedByEpochMigration: false,
    identityContract: "Notes sidecar is adjacent to its session JSONL locator; sessionId remains the durable identity.",
    siteRules: rules(
      ["lib/tools/context-notes-tool.ts"],
      "Writes the per-session context-notes markdown sidecar (atomic tmp+rename)."
    )
  }),
  defineStore({
    id: "session-files",
    ownerModule: "lib/session-files/session-file-registry.ts",
    pathPatterns: ["session-files/{sessionHash}"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("lib/session-files/session-file-registry.ts", "SessionFile payload naming, v1 sidecar, ownership, and 72-hour cold-session cleanup protocol"),
    openEntry: ["SessionFileRegistry"],
    checkpointPolicy: "Checkpoint sidecar and payload bytes together; exclude expired cold-session cache entries.",
    restorePolicy: "Restore by sessionId ownership and rebuild locators through SessionFileRegistry.",
    identityContract: "SessionFile ID and owning sessionId are identities; the hash directory and payload paths are locators.",
    siteRules: rules([
      "lib/session-files/bridge-inbound-files.ts",
      "lib/session-files/browser-screenshot-file.ts",
      "lib/session-files/session-file-registry.ts",
      "hub/index.ts"
    ], "Stages payload bytes or updates SessionFile sidecars under the managed cache.")
  }),
  defineStore({
    id: "session-drafts-and-projects",
    ownerModule: "core/input-drafts-store.ts",
    pathPatterns: ["input-drafts.v1.json", "user/session-projects.json"],
    format: "json",
    schemaSource: runtimeSource("core/input-drafts-store.ts", "InputDraftsStore and SessionProjectCatalogStore normalizers"),
    openEntry: ["new InputDraftsStore", "new SessionProjectCatalogStore"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    identityContract: "Draft/project records are keyed by sessionId; any saved sessionPath is a locator only.",
    siteRules: [
      ...rules(["core/input-drafts-store.ts", "core/session-project-catalog-store.ts"], "Owns session drafts or project associations."),
      ...rules(["core/engine.ts"], "Constructs draft/project stores.", ["persistent-store-constructor"], "(?:InputDraftsStore|SessionProjectCatalogStore)")
    ]
  }),
  defineStore({
    id: "conversation-map-layout",
    ownerModule: "server/routes/conversation-map.ts",
    pathPatterns: ["conversation-map.json"],
    format: "json",
    schemaSource: runtimeSource("server/routes/conversation-map.ts", "conversation-map layout normalizer (GET) and validated atomic writer (PUT)"),
    openEntry: ["createConversationMapRoute"],
    identityContract: "Layout entries are keyed by map card id; session/entry ids embedded in keys are references to session-owned data, not identities owned here.",
    siteRules: rules(["server/routes/conversation-map.ts"], "Reads and atomically writes the conversation-map canvas layout file.")
  }),
  defineStore({
    id: "channels",
    ownerModule: "lib/channels/channel-store.ts",
    pathPatterns: ["channels/{channelId}.yaml"],
    format: "yaml",
    schemaSource: runtimeSource("lib/channels/channel-store.ts", "channel YAML parser, serializer, and generation contract"),
    openEntry: ["ChannelManager", "ChannelStore"],
    identityContract: "channelId is durable; YAML filename is its canonical locator.",
    siteRules: [
      ...rules(["core/channel-manager.ts", "lib/channels/channel-store.ts"], "Creates the channel root or writes channel generations."),
      ...rules(["core/engine.ts"], "Creates the channel root.", ["mkdir"], "channelsDir")
    ]
  }),
  defineStore({
    id: "desk-activity",
    ownerModule: "lib/desk/activity-store.ts",
    pathPatterns: ["agents/{agentId}/desk/activities.json"],
    format: "mixed-directory",
    schemaSource: directorySource("lib/desk/activity-store.ts", "activity index and activity-session JSONL lifecycle"),
    openEntry: ["new ActivityStore", "DeskManager"],
    identityContract: "activityId/sessionId identify records; session files remain agent-owned locators.",
    siteRules: [
      ...rules(["lib/desk/activity-store.ts", "lib/desk/desk-manager.ts"], "Writes the activity index or activity session lifecycle state."),
      ...rules(["core/agent-manager.ts"], "Constructs an ActivityStore for an agent.", ["persistent-store-constructor"], "ActivityStore")
    ]
  }),
  defineStore({
    id: "cron-automation",
    ownerModule: "lib/desk/cron-store.ts",
    pathPatterns: [
      "agents/{agentId}/desk/cron-jobs.json",
      "agents/{agentId}/desk/cron-runs/{runId}.json",
      "studios/{studioId}/desk/cron-jobs.json",
      "studios/{studioId}/desk/cron-runs/{runId}.json"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("lib/desk/cron-store.ts", "CronStore job registry and run receipt protocol"),
    openEntry: ["new CronStore", "StudioCronService"],
    identityContract: "jobId and runId are scoped to their agentId or studioId owner.",
    siteRules: [
      ...rules(["lib/desk/cron-store.ts", "core/studio-cron-service.ts"], "Writes cron jobs or execution receipts."),
      ...rules(["core/agent.ts"], "Constructs an agent or studio CronStore.", ["persistent-store-constructor"], "CronStore")
    ]
  }),
  defineStore({
    id: "agent-authored-records",
    ownerModule: "lib/tools/experience.ts",
    pathPatterns: [
      "user/user.md",
      "agents/{agentId}/dm/{peerId}.md",
      "agents/{agentId}/experience.md",
      "agents/{agentId}/experience/{category}.md",
      "agents/{agentId}/diary/{date}.md"
    ],
    format: "markdown",
    schemaSource: directorySource("lib/tools/experience.ts", "user profile, DM, experience index, and diary Markdown conventions"),
    openEntry: ["user profile and agent tools"],
    identityContract: "Files are owned by userId or agentId; peer/category/date names define stable logical records.",
    siteRules: rules([
      "lib/tools/dm-tool.ts",
      "lib/tools/experience.ts",
      "lib/user-profile-store.ts",
      "lib/diary/diary-writer.ts"
    ], "Writes a user- or agent-authored Markdown record.")
  }),
  defineStore({
    id: "agent-phone",
    ownerModule: "lib/conversations/agent-phone-projection.ts",
    pathPatterns: [
      "agents/{agentId}/phone/conversations/{conversationId}.md",
      "agents/{agentId}/phone/session-runtime/{conversationId}.json"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("lib/conversations/agent-phone-projection.ts", "phone projection Markdown and session-runtime JSON readers"),
    openEntry: ["Agent Phone conversation runtime"],
    identityContract: "conversationId is identity within agentId; projection/runtime paths are locators.",
    siteRules: rules(["lib/conversations/agent-phone-projection.ts", "lib/conversations/agent-phone-runtime.ts"], "Writes Agent Phone projection or runtime state.")
  }),
  defineStore({
    id: "workflow-state",
    ownerModule: "lib/workflow/journal.ts",
    pathPatterns: [
      "agents/{agentId}/workflow-journals/{runId}.jsonl",
      "agents/{agentId}/workflow-sessions/{runId}",
      "workflow-activity.json"
    ],
    pathKind: "file",
    format: "mixed-directory",
    schemaSource: directorySource("lib/workflow/journal.ts", "WorkflowJournal append protocol and WorkflowActivityStore runtime reader"),
    openEntry: ["WorkflowJournal", "new WorkflowActivityStore"],
    identityContract: "runId is workflow identity; activity entries reference that ID, not the journal path.",
    siteRules: [
      ...rules(["lib/workflow/journal.ts", "lib/workflow-activity-store.ts", "lib/tools/workflow-tool.ts"], "Writes workflow journal, session, or activity state."),
      ...rules(["server/index.ts"], "Constructs the workflow activity store.", ["persistent-store-constructor"], "WorkflowActivityStore")
    ]
  }),
  defineStore({
    id: "subagent-state",
    ownerModule: "lib/subagent-run-store.ts",
    pathPatterns: ["subagent-runs.json", "subagent-threads.json"],
    format: "json",
    schemaSource: runtimeSource("lib/subagent-run-store.ts", "SubagentRunStore and SubagentThreadStore permissive runtime readers"),
    openEntry: ["new SubagentRunStore", "new SubagentThreadStore"],
    compatibility: "Current readers are permissive and repair legacy shapes; this inventory does not claim strict validation.",
    identityContract: "runId and threadId are durable; parent session references prefer sessionId and retain path only as a locator.",
    siteRules: [
      ...rules(["lib/subagent-run-store.ts", "lib/subagent-thread-store.ts", "lib/subagent-executor-metadata.ts"], "Writes subagent run, thread, or executor metadata."),
      ...rules(["server/index.ts"], "Constructs subagent stores.", ["persistent-store-constructor"], "Subagent(?:Run|Thread)Store")
    ]
  }),
  defineStore({
    id: "plugin-task-registry",
    ownerModule: "lib/task-registry.ts",
    pathPatterns: [".ephemeral/plugin-tasks.json"],
    format: "json",
    schemaSource: runtimeSource("lib/task-registry.ts", "TaskRegistry task/schedule normalization and persistence serializer"),
    openEntry: ["new TaskRegistry"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    checkpointPolicy: "Include persisted task/schedule metadata when plugin recovery must survive the checkpoint; handlers remain runtime-only.",
    restorePolicy: "Load through TaskRegistry, then let each plugin register its handler before recovery is attempted.",
    identityContract: "taskId/scheduleId are durable registry keys; parent session ownership prefers sessionId.",
    siteRules: [
      ...rules(["lib/task-registry.ts"], "Persists plugin task and schedule metadata."),
      ...rules(["core/engine.ts"], "Constructs the plugin task registry.", ["persistent-store-constructor"], "TaskRegistry")
    ]
  }),
  defineStore({
    id: "deferred-result-state",
    ownerModule: "lib/deferred-result-store.ts",
    pathPatterns: [".ephemeral/deferred-tasks.json"],
    format: "json",
    schemaSource: runtimeSource("lib/deferred-result-store.ts", "DeferredResultStore normalization, seven-day cleanup, and atomic serializer"),
    openEntry: ["new DeferredResultStore"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    checkpointPolicy: "Include undelivered deferred results needed for restart notification; expired/delivered entries may be pruned.",
    restorePolicy: "Load through DeferredResultStore so cleanup and delivery flags are applied before events resume.",
    identityContract: "taskId is durable; session ownership prefers sessionId and retains sessionPath only as a locator.",
    siteRules: [
      ...rules(["lib/deferred-result-store.ts"], "Persists deferred task/result delivery state."),
      ...rules(["server/index.ts"], "Constructs the deferred result store.", ["persistent-store-constructor"], "DeferredResultStore")
    ]
  }),
  defineStore({
    id: "loop-state",
    ownerModule: "lib/loop/loop-store.ts",
    pathPatterns: [".ephemeral/loop-state.json"],
    format: "json",
    schemaSource: runtimeSource("lib/loop/loop-store.ts", "LoopStore schemaVersion-1 sessionId-keyed record shape, corrupt-file quarantine, and atomic serializer"),
    openEntry: ["new LoopStore"],
    migrationEntry: [],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    checkpointPolicy: "Include running/paused loop records so restart recovery can re-arm alarms; terminal records may be pruned.",
    restorePolicy: "Load through LoopStore so corrupt-file quarantine and record normalization apply before recovery runs.",
    identityContract: "sessionId is the loop key for desktop and bridge targets alike; bridge sessionKey and any session path are delivery locators only.",
    siteRules: [
      ...rules(["lib/loop/loop-store.ts"], "Persists sessionId-keyed loop state."),
      ...rules(["server/index.ts"], "Constructs the loop store.", ["persistent-store-constructor"], "LoopStore")
    ]
  }),
  defineStore({
    id: "terminal-session-state",
    ownerModule: "lib/terminal/terminal-session-manager.ts",
    pathPatterns: [
      ".ephemeral/terminal-sessions/{terminalId}.json",
      ".ephemeral/terminal-sessions/{terminalId}.jsonl"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("lib/terminal/terminal-session-manager.ts", "terminal metadata JSON plus ordered transcript JSONL protocol"),
    openEntry: ["new TerminalSessionManager"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    checkpointPolicy: "Preserve metadata and transcript together only for diagnostic continuity; a live PTY handle is never checkpointed.",
    restorePolicy: "Load through TerminalSessionManager; persisted running entries are historical metadata, not resumable processes.",
    compatibility: "sessionId and toolCallId are optional additive metadata fields. Older entries without them still load; ownership resolves from the persisted sessionPath when no stable id is present, and chat navigation falls back to terminalId when no toolCallId is present.",
    identityContract: "terminalId identifies metadata/transcript; session ownership must resolve to sessionId before keyed access.",
    siteRules: rules(["lib/terminal/terminal-session-manager.ts"], "Writes terminal metadata or appends ordered transcript chunks.")
  }),
  defineStore({
    id: "skill-translation-cache",
    ownerModule: "lib/skills/skill-name-translation-cache.ts",
    pathPatterns: [".ephemeral/skill-name-translations.json"],
    format: "json",
    schemaSource: runtimeSource("lib/skills/skill-name-translation-cache.ts", "CACHE_VERSION and translation normalization"),
    openEntry: ["skill name translation lookup"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; translations are a versioned cache.",
    restorePolicy: "Delete and regenerate when CACHE_VERSION or normalization becomes incompatible.",
    affectedByEpochMigration: false,
    identityContract: "Cache keys derive from skill names/content; the file path is not a durable identity.",
    siteRules: rules(["lib/skills/skill-name-translation-cache.ts"], "Atomically updates the versioned skill translation cache.")
  }),
  defineStore({
    id: "knowledge-database",
    ownerModule: "lib/knowledge/knowledge-store.ts",
    pathPatterns: [
      "knowledge/knowledge.db",
      "knowledge/knowledge.db-wal",
      "knowledge/knowledge.db-shm"
    ],
    format: "sqlite",
    schemaSource: {
      kind: "sqlite-runtime",
      module: "lib/knowledge/knowledge-store.ts",
      contract: "KnowledgeStore runtime DDL, transactional migration, and store-local PRAGMA user_version"
    },
    openEntry: ["new KnowledgeStore", "new KnowledgeManager"],
    migrationEntry: ["KnowledgeStore store-local migrations"],
    checkpointPolicy: "Checkpoint with the managed source snapshots and citation-grade parse artifacts from the same Knowledge generation.",
    restorePolicy: "Restore through KnowledgeManager before queries resume; validate SQLite user_version and referenced managed bytes together.",
    compatibility: "schema v6 adds notebook config columns (embedding_model_ref/rerank_model_ref inherit the global preference when NULL; chunk_target_chars/retrieval_top_k fall back to built-in defaults 1200/12) and the ingestion_jobs queue table, and drops the V3-V5 research tables in the same transaction; research rows were derived artifacts, so the V1-V2 source-of-truth tables migrate untouched. schema v7 adds ingestion_jobs.progress_done (NOT NULL DEFAULT 0) and progress_total (NULL = embed phase not reached) for embedding progress; existing rows backfill 0/NULL and no other persisted shape changes. schema v8 is a pure data migration: retrieval_top_k is reset to NULL for all active notebooks (NULL = uncapped retrieval; the DDL DEFAULT 12 from v6 is a legacy artifact new notebooks no longer receive). schema v9 (P0 index identity) is purely additive: it creates the chunk_profiles and retrieval_profiles identity registries plus the notebooks.retrieval_profile_id binding column, and backfills chunk_profiles from ingestion_jobs.chunker_config_id history (unresolvable hashes become profile_type='legacy' rows with NULL config, never fabricated); no existing column or index/vector data is touched and no re-embedding is triggered. schema v10 adds ingestion_jobs.embedding_stats (JSON, NULL = embed phase never ran); schema v11 adds the KnowledgeTurnScope tables knowledge_turn_scopes and knowledge_turn_scope_sources. schema v12 (lifecycle governance) is additive: sources.orphaned_at marks zero-membership orphans for retention-based GC, ingestion_jobs.cancelled_at marks jobs cancelled by explicit source deletion (the status CHECK cannot gain a 'cancelled' value, so cancellation reuses the failed terminal state plus this explicit column, and requeue rejects cancelled rows), and a partial unique index enforces one active job per (notebook_id, source_id) after collapsing pre-existing duplicate active rows to failed with an explicit error. schema v13 (Phase 7 coverage planner) is purely additive: the knowledge_coverage_plans table persists structured KnowledgeCoveragePlan results only (intent/coverage_mode/requires_completeness/scope_level/sub_queries_json/confidence/matched_rule_ids_json/classifier_used/degrade_reason plus a nullable turn_scope_id reference); no chain-of-thought or raw model output is ever stored, and no existing table or row changes. schema v14 (Phase 9 exhaustive coverage execution) is purely additive: the coverage_runs and coverage_shards tables persist the frozen coverage manifest identity (manifest_hash plus manifest_json with unit texts for resume), deterministic shard rows with attempt counts, and worker ShardResult JSON only (structured findings with provenance; no chain-of-thought); recovery reuses completed shard results and resets running shards to pending, and no existing table or row changes. schema v15 (EvidenceManifest, task spec 67) is purely additive: the evidence_manifests and evidence_manifest_entries tables persist per-answer evidence identity chains only (turn scope/session/turn/coverage mode references plus per-source snapshot/artifact/chunk-profile/chunk-and-vector-variant ids, neighbor ids, block offsets, and citation labels); no chunk text, chain-of-thought, or model output is ever stored, entries are re-verified server-side against the frozen turn-scope source set on insert, and orphan GC plus deleteSource now skip sources referenced by any manifest; no existing table or row changes. schema v16 (Phase 12 ProcessingArtifact pipeline, task spec 58/59/69) is purely additive: the processing_artifacts table persists binary-format conversion generations keyed by the processor identity quadruple (content_snapshot_id, processor_id, processor_version, processor_config_hash) with fidelity, output locator, locatorMap JSON and warnings; parse_artifacts gains fidelity (legacy rows default to 'citation_grade') and processing_artifact_id; notebook_sources gains directory organization path columns (relative_path/folder_node/display_order, NULL = no directory context); no existing table, row, or index/vector data is touched and no re-processing is triggered. schema v18 \u65B0\u589E\u4E03\u5F20\u7814\u7A76\u53F0\u8D26\u8868\uFF1Aknowledge_research_runs\u3001knowledge_evidence_needs\u3001knowledge_research_rounds\u3001knowledge_research_read_receipts\u3001knowledge_evidence_items\u3001knowledge_need_evidence\u3001knowledge_research_actions\uFF1B\u5EFA\u8868\u4E0E user_version \u540C\u4E8B\u52A1\u63D0\u4EA4\u3002\u4FDD\u7559\u65E2\u6709\u8D44\u6599\u548C\u6D3E\u751F\u7D22\u5F15\uFF0C\u65B0\u589E\u8BB0\u5F55\u901A\u8FC7\u5916\u952E\u4E0E\u539F\u59CB\u6765\u6E90\u5B9A\u4F4D\u5173\u8054\uFF0C\u7EA6\u675F\u72B6\u6001\u3001\u6570\u91CF\u3001\u504F\u79FB\u3001\u6458\u8981\u683C\u5F0F\u548C JSON \u5F62\u72B6\u3002\u9605\u8BFB\u51ED\u636E\u53EA\u5B58\u5B9A\u4F4D\u4E0E hash\uFF0C\u8BC1\u636E\u8868\u53EA\u4FDD\u5B58\u7ECF\u5BBF\u4E3B\u6838\u5B9E\u7684\u77ED\u5F15\u6587\uFF0C\u52A8\u4F5C\u53EA\u4FDD\u5B58\u7ED3\u6784\u5316\u8BF7\u6C42/\u7ED3\u679C\u6458\u8981\uFF0C\u4E0D\u4FDD\u5B58\u5B8C\u6574\u63D0\u793A\u3001\u6A21\u578B\u539F\u59CB\u56DE\u7B54\u6216\u9690\u85CF\u601D\u8003\u3002 schema v19 \u4EC5\u65B0\u589E knowledge_completeness_checks\u3001knowledge_completeness_units\u3001knowledge_completeness_unit_evidence\u3001knowledge_completeness_coverage_runs \u56DB\u5F20\u5B8C\u6574\u6027\u8BB0\u5F55\u8868\uFF1B\u5728\u540C\u4E00\u4E8B\u52A1\u4E2D\u5EFA\u8868\u548C\u66F4\u65B0\u7248\u672C\uFF0C\u5DF2\u6709\u8D44\u6599\u3001\u539F\u6587\u51ED\u636E\u53CA\u7814\u7A76\u8BC1\u636E\u9010\u884C\u4FDD\u6301\u4E0D\u53D8\u3002\u65B0\u8868\u4EC5\u4FDD\u5B58\u8BA1\u6570\u3001\u6838\u67E5\u72B6\u6001\u548C\u65E2\u6709\u8D44\u6599/\u8BC1\u636E/\u8986\u76D6\u8FD0\u884C\u7684\u8EAB\u4EFD\u5173\u8054\uFF0C\u7EA6\u675F\u552F\u4E00\u6027\u3001\u5F15\u7528\u3001\u504F\u79FB\u548C\u5B8C\u6574\u6027\u8BA1\u6570\uFF0C\u4E0D\u590D\u5236\u6B63\u6587\u6216\u6A21\u578B\u8F93\u51FA\u3002",
    identityContract: "One Knowledge database belongs to one canonical LINGXI_HOME; Notebook, Source, Snapshot, and later run IDs are durable identities, while storage paths are relative locators.",
    siteRules: rules(
      ["lib/knowledge/knowledge-store.ts"],
      "Creates the private Knowledge directory and opens the store-local SQLite database.",
      ["mkdir", "database-open"]
    )
  }),
  defineStore({
    id: "knowledge-source-snapshots",
    ownerModule: "lib/knowledge/knowledge-manager.ts",
    pathPatterns: ["knowledge/sources/**"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource(
      "lib/knowledge/knowledge-manager.ts",
      "ContentSnapshot relative locator, immutable byte publication, SHA-256 verification, and rollback protocol"
    ),
    openEntry: ["new KnowledgeManager", "KnowledgeManager.importFile", "KnowledgeManager.readContentSnapshot"],
    checkpointPolicy: "Checkpoint every referenced immutable snapshot byte together with knowledge.db; unreferenced staging files are excluded.",
    restorePolicy: "Restore immutable bytes at their relative locators, then verify byte length and SHA-256 before serving citations or analysis.",
    identityContract: "snapshotId and SHA-256 identify immutable captured bytes; the managed relative path is a locator and the external origin path is metadata only.",
    siteRules: rules(
      ["lib/knowledge/knowledge-manager.ts"],
      "Publishes or rolls back only Knowledge-managed immutable source snapshot bytes.",
      ["mkdir", "write-file", "rename", "remove-path"],
      "sourcesRoot|sourceDirectory|temporaryPath|snapshotPath|handle[.]writeFile"
    )
  }),
  defineStore({
    id: "knowledge-parse-artifacts",
    ownerModule: "lib/knowledge/knowledge-manager.ts",
    pathPatterns: ["knowledge/artifacts/**"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource(
      "lib/knowledge/knowledge-manager.ts",
      "ParseArtifact generation directories referenced by knowledge.db and never treated as source content versions"
    ),
    openEntry: ["new KnowledgeManager"],
    checkpointPolicy: "Checkpoint citation-grade parse artifacts that are referenced by knowledge.db together with their ContentSnapshot generations.",
    restorePolicy: "Restore only artifacts whose parser identity and source snapshot still validate; rebuild derived semantic views when compatibility requires it.",
    identityContract: "parseArtifactId identifies one parser/config generation for one immutable ContentSnapshot; it never changes the ContentSnapshot identity.",
    siteRules: rules(
      ["lib/knowledge/knowledge-manager.ts"],
      "Creates, atomically publishes, or rolls back citation-grade ParseArtifact generations.",
      ["mkdir", "write-file", "rename", "remove-path"],
      "artifactsRoot|artifactDirectory|artifactTemporaryPath|artifactPath|artifactHandle[.]writeFile"
    )
  }),
  defineStore({
    id: "knowledge-processing-artifacts",
    ownerModule: "lib/knowledge/knowledge-manager.ts",
    pathPatterns: ["knowledge/processed/**"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource(
      "lib/knowledge/knowledge-manager.ts",
      "ProcessingArtifact output files (binary office formats converted to structured text) referenced by knowledge.db processing_artifacts rows, with locatorMap held in the database"
    ),
    openEntry: ["new KnowledgeManager", "KnowledgeManager.parseSource"],
    checkpointPolicy: "Checkpoint ready ProcessingArtifact outputs that are referenced by knowledge.db together with their ContentSnapshot generations; unreferenced staging files are excluded.",
    restorePolicy: "Restore only outputs whose processor identity and content snapshot still validate; re-run the processor when compatibility requires it.",
    compatibility: "schema v16 introduces the processing_artifacts table (processor identity quadruple, fidelity, output locator, locatorMap JSON) and these managed output files; parse_artifacts gains fidelity plus processing_artifact_id back-references, and notebook_sources gains directory organization paths (relative_path/folder_node/display_order).",
    identityContract: "(contentSnapshotId, processorId, processorVersion, processorConfigHash) identifies one conversion generation; outputs are deterministic projections of immutable snapshot bytes and never change ContentSnapshot identity.",
    siteRules: rules(
      ["lib/knowledge/knowledge-manager.ts"],
      "Creates, atomically publishes, reads back, or rolls back ProcessingArtifact output files.",
      ["mkdir", "write-file", "rename", "remove-path"],
      "processedRoot|processedDirectory|processedTemporaryPath|processedOutputPath|processedOutputFile"
    )
  }),
  defineStore({
    id: "knowledge-indexes",
    ownerModule: "lib/knowledge/knowledge-index-store.ts",
    pathPatterns: ["knowledge/indexes/**"],
    pathKind: "tree",
    format: "binary-cache",
    schemaSource: directorySource(
      "lib/knowledge/knowledge-index-store.ts",
      "Lexical SQLite schema v3, portable vector SQLite schema v3, and rebuildable ANN catalog schema v1. ANN files derive from retained portable vector BLOBs; ANN failure never deletes those BLOBs"
    ),
    openEntry: ["new KnowledgeManager", "new KnowledgeIndexStore", "new PortableVectorIndexAdapter", "new AnnIndexStore"],
    migrationEntry: ["KnowledgeIndexStore runtime DDL", "PortableVectorIndexAdapter runtime DDL", "AnnIndexStore runtime DDL"],
    protocolModules: ["lib/knowledge/vector-index-adapter.ts", "lib/knowledge/ann-index-store.ts", "lib/knowledge/usearch-vector-backend.ts", "lib/knowledge/vector-search-backend-factory.ts"],
    epochPolicy: "regenerable",
    checkpointPolicy: "Portable vector BLOBs retain ingestion batch checkpoints and remain the recovery input for ANN; regenerable ANN files are not a checkpoint authority. Epoch checkpoint exclusion for this projection store remains unchanged.",
    restorePolicy: "ANN corruption rebuilds only knowledge-ann.db and knowledge-ann files from retained portable vector BLOBs. Lexical projections rebuild from parse artifacts; never use an ANN file as the sole vector recovery source.",
    affectedByEpochMigration: false,
    compatibility: "P3-04 \u5C06 FTS v3 \u589E\u91CF\u5347\u7EA7\u4E3A v4\uFF1A\u65B0\u589E\u6765\u6E90/\u7AE0\u8282\u4E0E\u5BF9\u5E94\u5168\u6587\u7D22\u5F15\u3001\u7247\u6BB5\u8FFD\u52A0\u53EF\u7A7A section_id\uFF1B\u65E7\u884C\u3001\u65E7 v2 \u5206\u5757\u914D\u7F6E\u548C\u5411\u91CF\u4FDD\u7559\uFF0C\u540E\u53F0\u5206\u6279\u5EFA\u7ACB v3 512-token \u7247\u6BB5\uFF0C\u672A\u5C31\u7EEA\u65F6\u7EE7\u7EED\u8BFB\u65E7\u7D22\u5F15\u3002\u8FC1\u79FB\u5931\u8D25\u6574\u7B14\u56DE\u6EDA\u4E14\u4FDD\u7559\u65E7\u5E93\u3002P1 adds chunk_index_variant_metadata (FTS v3) and a separate ann_variants catalog (ANN v1); portable vector schema stays v3 and retains BLOBs for exact fallback and ANN rebuild. Historical knowledge-fts.db schema v2 and knowledge-vector.db schema v2 (P0 index identity) migrate v1 in a single transaction each: chunk identity moves from (parse_artifact_id, ordinal) to ChunkIndexVariant (parse_artifact_id, chunk_profile_hash) + ordinal via chunk_index_variants backfill (artifact_indexes is retired after backfill), and vector identity moves from (parse_artifact_id, model_key) to VectorIndexVariant (chunk_index_variant_id, model_key) via vector_index_variants backfill (vector_artifacts is retired after backfill). Rows whose chunk profile cannot be resolved are filed under the explicit legacy_unknown identity instead of being dropped; migrations only establish identities and remap existing rows, never re-chunk or re-embed.",
    identityContract: "Index generation IDs are disposable projection identities; ChunkIndexVariant (parseArtifactId, chunkProfileHash) and VectorIndexVariant (chunkIndexVariantId, modelKey) are deterministic derived identities, while Notebook, Source, Snapshot, Artifact, Evidence, and Claim facts never originate here.",
    siteRules: [
      ...rules(
        ["lib/knowledge/knowledge-manager.ts"],
        "Creates the regenerable Knowledge index root.",
        ["mkdir"],
        "indexesRoot"
      ),
      ...rules(
        ["lib/knowledge/knowledge-index-store.ts"],
        "Creates and removes only the regenerable lexical SQLite index files.",
        ["mkdir", "remove-path"]
      ),
      ...rules(
        ["lib/knowledge/vector-index-adapter.ts"],
        "Creates and removes only the regenerable vector SQLite cache files.",
        ["mkdir", "remove-path"]
      ),
      ...rules(
        ["lib/knowledge/ann-index-store.ts", "lib/knowledge/usearch-vector-backend.ts", "lib/knowledge/vector-search-backend-factory.ts"],
        "Creates and replaces only ANN catalog and derived index files; never deletes portable vector BLOBs.",
        ["mkdir", "remove-path", "rename", "database-open"]
      )
    ]
  }),
  defineStore({
    id: "usage-ledger",
    ownerModule: "lib/llm/usage-ledger.ts",
    pathPatterns: ["usage-ledger.json"],
    format: "json",
    schemaSource: runtimeSource("lib/llm/usage-ledger.ts", "UsageLedger reader and atomic serializer"),
    openEntry: ["new UsageLedger"],
    identityContract: "Usage buckets are keyed by their runtime dimensions inside one LINGXI_HOME ledger.",
    siteRules: rules(["lib/llm/usage-ledger.ts"], "Writes the usage ledger.")
  }),
  defineStore({
    id: "model-observability-db",
    ownerModule: "lib/llm/model-observability-schema.ts",
    pathPatterns: [
      "model-observability/observability.sqlite",
      "model-observability/observability.sqlite-wal",
      "model-observability/observability.sqlite-shm"
    ],
    format: "sqlite",
    schemaSource: { kind: "sqlite-runtime", module: "lib/llm/model-observability-schema.ts", contract: "Model Observatory runtime DDL, PRAGMA user_version open/migration contract, and disable-on-failure semantics" },
    openEntry: ["openModelObservabilityDatabase", "installModelObservabilityPersistence"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    epochPolicy: "compatible",
    checkpointPolicy: "Excluded from business data-epoch migration checkpoints: the observatory is not required to restore Agent/Session correctness and may be very large. Its own schema evolves through SQLite user_version migrations, never through DATA_EPOCH transitions.",
    restorePolicy: "Never restored by epoch migrations. Observability rows that reference sessions rolled back by an epoch restore are allowed historical facts; query surfaces report them as session-unavailable rather than deleting them.",
    affectedByEpochMigration: false,
    identityContract: "One global observatory database per LINGXI_HOME; callId/attemptId/traceId are runtime observation identities correlated with (but never owned by) business epoch identities.",
    siteRules: [
      ...rules(["lib/llm/model-observability-schema.ts"], "Opens or creates the observatory database.", ["database-open", "mkdir"]),
      ...rules(["lib/llm/model-observability-read-database.ts"], "Opens the observatory database strictly read-only (query side; fileMustExist, never creates or migrates).", ["database-open"]),
      ...rules(["lib/llm/model-observability-persistence.ts"], "Creates the observability store directory while tightening at-rest permissions.", ["mkdir"]),
      ...rules(["lib/llm/model-observability-testing.ts"], "Removes a throwaway test-harness copy of the observatory store tree under os.tmpdir().", ["remove-path"])
    ]
  }),
  defineStore({
    id: "model-observability-blobs",
    ownerModule: "lib/llm/model-observability-blob-store.ts",
    pathPatterns: ["model-observability/blobs/**"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("lib/llm/model-observability-blob-store.ts", "observatory blob shard layout, random blobId file names, atomic staging rename, owner-only permissions, and ref-count-based GC protocol"),
    openEntry: ["createModelObservabilityBlobStore"],
    firstPossibleOpenPhase: "engine_construct",
    firstPossibleWritePhase: "engine_construct",
    epochPolicy: "compatible",
    checkpointPolicy: "Excluded from epoch checkpoints: blob bytes are privileged observability captures whose lifetime is governed by payload retention and ref-count GC, not by business epoch transitions.",
    restorePolicy: "Never restored by epoch migrations; orphan blob files left by crashes are reclaimed through the observatory's own grace-period recovery, never replayed as business state.",
    affectedByEpochMigration: false,
    identityContract: "blobId is a random observatory identity (mb_<random>); files never carry original filenames and metadata keeps only mime type and byte length.",
    siteRules: rules(
      ["lib/llm/model-observability-blob-store.ts"],
      "Writes, renames, or removes observatory blob files under the private blob tree.",
      ["mkdir", "write-file", "rename", "remove-path"]
    )
  }),
  defineStore({
    id: "mcp-config",
    ownerModule: "core/mcp/manager.ts",
    pathPatterns: ["plugin-data/mcp"],
    pathKind: "tree",
    format: "json",
    schemaSource: runtimeSource(
      "core/mcp/manager.ts",
      "normalizeMcpConfig read-time normalization (servers/connectors alias, auth and OAuth field defaults, per-connector permission policy defaults, deferred-loading defaults, connection lifecycle defaults)"
    ),
    openEntry: ["Engine constructor via McpManager"],
    migrationEntry: [
      "normalizeMcpConfig read-time normalization (servers\u2192connectors alias)",
      "normalizeMcpConfig read-time permission policy defaults (permissionMode/toolPermissions/trustReadOnlyHint)",
      "normalizeMcpConfig read-time deferred-loading defaults (deferEnabled true, deferThreshold 10)",
      "normalizeMcpConfig read-time connection lifecycle defaults (lifecycle keep-alive, idleTimeoutMinutes null \u2192 mode default)"
    ],
    checkpointPolicy: "Single JSON config; checkpoint the whole file.",
    restorePolicy: "Restore the whole file; read-time normalization absorbs older shapes.",
    // The directory name predates the move to core/mcp and stays put: it is
    // where every existing install already keeps its connector config.
    identityContract: "core/mcp owns plugin-data/mcp exclusively.",
    siteRules: rules(["core/mcp/manager.ts"], "Sole writer of plugin-data/mcp/config.json.")
  }),
  defineStore({
    id: "plugin-runtime-data",
    ownerModule: "core/plugin-config.ts",
    pathPatterns: ["plugin-data/{pluginId}"],
    pathExclusions: ["plugin-data/office/jobs", "plugin-data/office/generated", "plugin-data/mcp"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: {
      kind: "narrow-exemption",
      reason: "Each plugin owns its manifest-declared data contract; third-party data requires per-plugin migrationVersion registration before epoch migration.",
      expiresOn: "2026-12-31"
    },
    openEntry: ["PluginManager activation with plugin-scoped dataDir"],
    migrationEntry: ["plugin manifest/migrationVersion hook; no host-wide implicit migration"],
    checkpointPolicy: "Checkpoint by pluginId and declared migrationVersion; never infer one schema for plugin-data/**.",
    restorePolicy: "Restore only when the same pluginId and a compatible manifest/migrationVersion are available.",
    identityContract: "pluginId from the active manifest owns exactly its plugin-data subtree.",
    exemption: {
      reason: "Dynamic plugin schemas must be registered by pluginId and migrationVersion before a coordinated data migration consumes them.",
      expiresOn: "2026-12-31"
    },
    siteRules: [
      // Pinned to the owner-only kind on purpose: plugin configuration holds
      // whatever credentials a plugin asks its user for, so a write here that
      // reverts to the generic writer must fail the census rather than pass as
      // ordinary plugin data.
      ...rules(
        ["core/plugin-config.ts"],
        "Writes the plugin configuration file for the active pluginId.",
        ["secret-write", "mkdir"]
      ),
      ...rules([
        "core/media/download.ts",
        "core/media/local-cli-wrapper.ts",
        "core/media/task-store.ts",
        "core/media/universal-media-manager.ts",
        "core/media-adapters/agnes.ts",
        "core/media-adapters/speech.ts",
        "plugins/jimeng-cli/adapters/dreamina.ts"
      ], "Writes data within the active pluginId-scoped data directory.")
    ]
  }),
  defineStore({
    id: "skill-state",
    ownerModule: "lib/skill-bundles/store.ts",
    pathPatterns: ["skills/{skillName}", "skill-bundles.json", ".ephemeral/skill-bundle-exports/{token}"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("lib/skills/skill-package-installer.ts", "SKILL.md package, bundle registry, and export staging protocols"),
    openEntry: ["skill loader", "SkillBundleStore"],
    firstPossibleOpenPhase: "first_run_seed",
    firstPossibleWritePhase: "first_run_seed",
    identityContract: "skillName is identity within the user skill root; bundle IDs are registry keys.",
    siteRules: [
      ...rules(["lib/skill-bundles/package-service.ts", "lib/skill-bundles/store.ts", "lib/skills/skill-package-installer.ts", "server/utils/uploaded-skill-package.ts"], "Installs, exports, uploads, or records skill packages."),
      ...rules(["core/engine.ts"], "Creates the user skill root.", ["mkdir"], "skillsDir"),
      ...rules(["lib/character-cards/service.ts"], "Installs, rolls back, or renames a skill imported from a character card.", ["mkdir", "write-file", "remove-path"], "(?:engine[.]userSkillsDir|skillMdPath|item[.]dir)"),
      ...rules(["lib/skills/skill-removal.ts"], "Removes installed user skills (single delete route and agent-delete cleanup share this module).", ["remove-path"], "rmSync\\(dir")
    ]
  }),
  defineStore({
    id: "operational-checkpoints",
    ownerModule: "lib/checkpoint-store.ts",
    pathPatterns: ["checkpoints/{checkpointId}"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("lib/checkpoint-store.ts", "file-copy checkpoint manifest and session-manifest SQLite checkpoint protocol"),
    openEntry: ["CheckpointStore", "createSessionManifestCheckpoint"],
    epochPolicy: "compatible",
    checkpointPolicy: "This is the existing operational checkpoint facility, not an epoch checkpoint system.",
    restorePolicy: "Existing restore APIs copy declared files; they provide no cross-store epoch transaction guarantee.",
    affectedByEpochMigration: false,
    identityContract: "checkpointId names an operational snapshot; it does not establish a global data epoch.",
    siteRules: rules(["lib/checkpoint-store.ts", "core/session-manifest/checkpoint.ts"], "Writes or restores an existing operational checkpoint.")
  }),
  defineStore({
    id: "runtime-diagnostics",
    ownerModule: "lib/debug-log.ts",
    pathPatterns: [
      "logs/{timestamp}.log",
      "browser-worker.log",
      "switch-error.log",
      "user/browser-sessions.json",
      "bridge/wechat/sync-{accountHash}.json",
      "bridge/wechat/context-{accountHash}.json"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("lib/debug-log.ts", "debug logs, browser cold state, switch diagnostics, and WeChat cache readers"),
    openEntry: ["runtime logging and bridge/browser startup"],
    epochPolicy: "compatible",
    checkpointPolicy: "Exclude rotating logs; browser/bridge cold state may be included as best-effort compatible cache.",
    restorePolicy: "Logs are append history; cache records may be dropped if incompatible.",
    affectedByEpochMigration: false,
    identityContract: "Diagnostic filenames are locators; browser and bridge cache keys are runtime-owned account/session keys.",
    siteRules: [
      ...rules(["lib/debug-log.ts", "lib/browser/browser-manager.ts", "lib/bridge/wechat-adapter.ts"], "Writes runtime diagnostics or recoverable cold state."),
      ...rules(["server/index.ts"], "Appends the browser worker log.", ["append-file"], "_bwsLogPath"),
      ...rules(["server/routes/sessions.ts"], "Appends switch failure diagnostics.", ["append-file"], "switch-error[.]log"),
      ...rules(["desktop/auto-updater.cjs"], "Appends desktop auto-update diagnostics.", ["mkdir", "append-file"], "(?:logDir|auto-update[.]log)")
    ]
  }),
  defineStore({
    id: "desktop-diagnostics",
    ownerModule: "desktop/src/shared/desktop-launch-diagnostics.cjs",
    pathPatterns: [
      "diagnostics/desktop-launch",
      "crash.log",
      "browser-cmd.log"
    ],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("desktop/src/shared/desktop-launch-diagnostics.cjs", "desktop launch, renderer, server-crash, and browser-command diagnostic logs"),
    openEntry: ["desktop/bootstrap.cjs", "desktop main-process diagnostics"],
    firstPossibleOpenPhase: "desktop_bootstrap",
    firstPossibleWritePhase: "desktop_bootstrap",
    epochPolicy: "compatible",
    checkpointPolicy: "Exclude best-effort diagnostic logs from epoch checkpoints.",
    restorePolicy: "Do not restore as application state; retain only as independent troubleshooting evidence.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "epoch-independent",
      reason: "Desktop diagnostics are append/replace troubleshooting evidence whose reader does not depend on the application data epoch.",
      unstampedHomeSafePaths: [
        { relativePath: "diagnostics/desktop-launch", kind: "tree" },
        { relativePath: "crash.log", kind: "file" },
        { relativePath: "browser-cmd.log", kind: "file" }
      ]
    },
    identityContract: "Diagnostic filenames are fixed locators and never provide user, session, or epoch identity.",
    siteRules: [
      ...rules(["desktop/bootstrap.cjs", "desktop/src/shared/launch-integrity.cjs", "desktop/src/shared/desktop-launch-diagnostics.cjs"], "Writes epoch-independent desktop launch diagnostics."),
      ...rules(["desktop/main.cjs"], "Writes desktop crash or browser-command diagnostics.", ["mkdir", "write-file", "append-file"], "(?:lingxiHome|crashLogPath|browser-cmd[.]log)")
    ]
  }),
  defineStore({
    id: "desktop-gpu-startup-state",
    ownerModule: "desktop/src/shared/gpu-startup-policy.cjs",
    pathPatterns: ["user/gpu-startup.json"],
    format: "json",
    schemaSource: runtimeSource("desktop/src/shared/gpu-startup-policy.cjs", "GPU startup state version, crash evidence, recovery mode, and legacy migration markers"),
    openEntry: ["resolveGpuStartupPolicy", "desktop GPU startup markers"],
    firstPossibleOpenPhase: "desktop_bootstrap",
    firstPossibleWritePhase: "desktop_bootstrap",
    epochPolicy: "compatible",
    checkpointPolicy: "Exclude or preserve independently as launch-recovery metadata; never use it as an epoch checkpoint.",
    restorePolicy: "Load only through the GPU startup policy reader; corrupt exact-migration evidence fails explicitly.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "epoch-independent",
      reason: "GPU launch recovery is interpreted entirely by the versioned desktop policy before the server exists.",
      unstampedHomeSafePaths: [
        { relativePath: "user/gpu-startup.json", kind: "file" }
      ]
    },
    identityContract: "One fixed GPU startup record belongs to one desktop installation data home.",
    siteRules: rules(["desktop/src/shared/gpu-startup-policy.cjs"], "Atomically writes desktop GPU startup state.", ["atomic-write"], "writeJson\\(getGpuStartupStatePath")
  }),
  defineStore({
    id: "desktop-win32-install-acl-heal-state",
    ownerModule: "desktop/src/shared/win32-install-acl-heal.cjs",
    pathPatterns: ["user/win32-install-acl-heal.json"],
    format: "json",
    schemaSource: runtimeSource(
      "desktop/src/shared/win32-install-acl-heal.cjs",
      "install ACL heal state version, per install-identity grant bookkeeping, recovery probe result, and ineffective probe count"
    ),
    openEntry: ["maybeHealWin32InstallAcl", "desktop install ACL heal bookkeeping"],
    firstPossibleOpenPhase: "desktop_bootstrap",
    firstPossibleWritePhase: "desktop_bootstrap",
    epochPolicy: "compatible",
    checkpointPolicy: "Exclude or preserve independently as launch-recovery metadata; never use it as an epoch checkpoint.",
    restorePolicy: "Load only through the install ACL heal reader; an unreadable record is treated as absent and the idempotent grant simply runs again.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "epoch-independent",
      reason: "Install-directory ACL repair runs before the server exists and is interpreted entirely by the versioned desktop module.",
      unstampedHomeSafePaths: [
        { relativePath: "user/win32-install-acl-heal.json", kind: "file" }
      ]
    },
    identityContract: "One heal record belongs to one desktop installation data home and is keyed inside the record by install directory and shell version.",
    siteRules: rules(
      ["desktop/src/shared/win32-install-acl-heal.cjs"],
      "Writes the desktop install ACL heal record through its temporary file and rename.",
      ["mkdir", "write-file", "rename"],
      "(?:filePath|tmpPath)"
    )
  }),
  defineStore({
    id: "desktop-window-version-state",
    ownerModule: "desktop/main.cjs",
    pathPatterns: [
      "user/window-state.json",
      "user/quick-chat-window-state.json",
      "user/last-seen-version.json",
      "last-update-version"
    ],
    format: "mixed-directory",
    schemaSource: directorySource("desktop/main.cjs", "desktop window bounds, release announcement bookmark, and update cache version marker readers"),
    openEntry: ["desktop window creation", "post-update announcement", "desktop update cache cleanup"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "compatible",
    checkpointPolicy: "Optional compatible shell metadata; it is not required for restoring durable agent/session state.",
    restorePolicy: "Read through each desktop owner; invalid or absent shell metadata is recreated from safe defaults.",
    affectedByEpochMigration: false,
    identityContract: "Each fixed path is shell-local presentation or update metadata, never durable application identity.",
    siteRules: [
      ...rules(["desktop/main.cjs"], "Writes desktop window bounds or the release-announcement version bookmark.", ["mkdir", "write-file"], "(?:lastSeenVersionPath|windowStatePath|quickChatWindowStatePath)"),
      ...rules(["desktop/auto-updater.cjs"], "Migrates or writes the current desktop update-cache version marker.", ["mkdir", "rename", "write-file"], "(?:versionFile|wrongFile, versionFile)")
    ]
  }),
  defineStore({
    id: "desktop-update-channel",
    ownerModule: "desktop/auto-updater.cjs",
    pathPatterns: ["update-channel.json"],
    format: "json",
    schemaSource: runtimeSource(
      "desktop/auto-updater.cjs",
      "update channel record version, activation flag, update feed address, and activation time (legacy records may still carry a device identifier and held invite codes, which are ignored)"
    ),
    openEntry: ["desktop update feed resolution", "activated channel feed selection"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "compatible",
    checkpointPolicy: "Optional compatible shell metadata; which update address this installation follows is not durable agent or session state.",
    restorePolicy: "Read only through the desktop updater reader; an unparsable or unknown-version record falls back to the default update address and reports the reason to the user instead of degrading silently.",
    affectedByEpochMigration: false,
    identityContract: "One record per desktop installation data home; the record only selects which update feed this installation follows.",
    siteRules: rules(
      ["desktop/auto-updater.cjs"],
      "Writes the desktop update channel record through its temporary file and rename.",
      ["write-file", "rename"],
      "updateChannel"
    )
  }),
  defineStore({
    id: "managed-runtime-caches",
    ownerModule: "lib/pi-sdk/search-tools.ts",
    pathPatterns: ["runtime/pi-sdk/bin/{toolName}", ".ephemeral/win32-sandbox-runtime/{fingerprint}"],
    pathKind: "tree",
    format: "binary-cache",
    schemaSource: directorySource("lib/pi-sdk/search-tools.ts", "managed search-tool binary and Windows sandbox runtime cache validation; ast-grep (sg/ast-grep) joins the same cache via the npm platform-package channel with sha512 integrity (lib/sandbox/ast-grep-binary.ts)"),
    openEntry: ["search tool first use", "Windows sandbox runtime preparation", "ast tool first use"],
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; validated caches are rebuilt or recopied from trusted sources.",
    restorePolicy: "Delete and regenerate on incompatibility.",
    affectedByEpochMigration: false,
    identityContract: "Cache key is tool/runtime fingerprint; cached paths are never persistent business identity.",
    siteRules: rules(["lib/pi-sdk/search-tools.ts", "lib/sandbox/win32-runtime-cache.ts", "lib/sandbox/ast-grep-binary.ts"], "Populates a Hana-owned regenerable runtime cache.")
  }),
  defineStore({
    id: "signed-artifacts",
    ownerModule: "shared/artifact-core/pointer-store.cjs",
    pathPatterns: ["artifacts"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("shared/artifact-core/pointer-store.cjs", "signed artifact pointers, verified receipts, quarantine, lock, and OTA state protocol"),
    openEntry: ["artifact activation and OTA check"],
    firstPossibleOpenPhase: "desktop_bootstrap",
    firstPossibleWritePhase: "desktop_bootstrap",
    epochPolicy: "compatible",
    checkpointPolicy: "Exclude downloaded payload caches; preserve only if signature/pointer verification remains valid.",
    restorePolicy: "Re-verify every pointer and artifact before activation.",
    affectedByEpochMigration: false,
    bootstrapSafety: {
      compatibility: "epoch-independent",
      reason: "Signed component activation is verified by artifact manifests and pointers before application data is opened.",
      unstampedHomeSafePaths: [
        { relativePath: "artifacts", kind: "tree" }
      ]
    },
    identityContract: "Signed digest/version identify artifacts; pointer and staging paths are locators.",
    siteRules: rules([
      "shared/artifact-core/activation.cjs",
      "shared/artifact-core/ota-core.cjs",
      "shared/artifact-core/pointer-store.cjs",
      "desktop/src/shared/artifact-gc.cjs",
      "desktop/src/shared/artifact-repair.cjs"
    ], "Writes, deletes, or repairs signed artifact activation and OTA state.")
  }),
  defineStore({
    id: "legacy-upload-cache",
    ownerModule: "server/routes/upload.ts",
    pathPatterns: ["uploads/{uploadName}"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("server/routes/upload.ts", "sessionless upload naming and age-based cleanup protocol"),
    openEntry: ["resolveUploadTarget without sessionPath"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; sessionless uploads are a short-lived compatibility cache.",
    restorePolicy: "Discard and ask the caller to upload again; age cleanup removes stale entries.",
    affectedByEpochMigration: false,
    identityContract: "Generated uploadName is a cache locator, not a durable file or session identity.",
    siteRules: rules(["server/routes/upload.ts"], "Expires a sessionless legacy upload cache entry.", ["remove-path"], "fullPath")
  }),
  defineStore({
    id: "character-card-staging",
    ownerModule: "lib/character-cards/service.ts",
    pathPatterns: [
      ".ephemeral/character-card-uploads/{fileName}",
      ".ephemeral/character-card-imports/{token}",
      ".ephemeral/character-card-exports/{token}"
    ],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("lib/character-cards/service.ts", "token-scoped import plan/package and export package protocol"),
    openEntry: ["character card upload, import planning, or export"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; plans and packages are request-scoped staging.",
    restorePolicy: "Discard incomplete tokens and restart import/export from the original package.",
    affectedByEpochMigration: false,
    identityContract: "Random token owns one staging subtree; staged paths never become agent or skill identities.",
    siteRules: [
      ...rules(["server/routes/character-cards.ts"], "Writes a bounded character-card upload package."),
      ...rules(["lib/character-cards/service.ts"], "Writes or removes a token-scoped character-card import/export plan or package.", ["mkdir", "write-file", "copy-file", "remove-path", "atomic-write"], "(?:filePath, JSON[.]stringify|packageRoot|stageDir|exportRoot)")
    ]
  }),
  defineStore({
    id: "desk-cover-upload-staging",
    ownerModule: "server/routes/desk.ts",
    pathPatterns: ["tmp/markdown-cover-uploads/{token}"],
    pathKind: "tree",
    format: "directory-tree",
    schemaSource: directorySource("server/routes/desk.ts", "bounded base64 cover upload temporary-directory lifecycle"),
    openEntry: ["writeUploadedCoverImage"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; each cover upload is request-scoped temporary input.",
    restorePolicy: "Delete the temporary directory and upload the cover again.",
    affectedByEpochMigration: false,
    identityContract: "Temporary directory token owns the upload bytes; the final Markdown attachment has separate workspace ownership.",
    siteRules: rules(["server/routes/desk.ts"], "Writes or removes a bounded temporary cover upload.", ["mkdir", "write-file", "remove-path"], "(?:uploadRoot|filePath, buffer|tempDir)")
  }),
  defineStore({
    id: "office-render-jobs",
    ownerModule: "plugins/office/lib/html-to-pdf.ts",
    pathPatterns: ["plugin-data/office/jobs", "plugin-data/office/generated"],
    pathKind: "tree",
    format: "mixed-directory",
    schemaSource: directorySource("plugins/office/lib/html-to-pdf.ts", "job.json/input.html request contract and generated PDF output lifecycle"),
    openEntry: ["renderHtmlToPdf"],
    firstPossibleOpenPhase: "runtime_ready",
    firstPossibleWritePhase: "runtime_ready",
    epochPolicy: "regenerable",
    checkpointPolicy: "Excluded; jobs are request-scoped and generated PDFs are re-creatable outputs.",
    restorePolicy: "Discard incomplete jobs; rerun rendering from source HTML when output is still needed.",
    affectedByEpochMigration: false,
    identityContract: "jobId scopes one render request; outputPath is a locator and SessionFile registration supplies durable session ownership.",
    siteRules: rules(["plugins/office/lib/html-to-pdf.ts"], "Writes an office render job input, receipt, or generated output.")
  })
]);
function exemption(id, ownerModule, sourceFile, reason, expiresOn, kinds = ALL_SITE_KINDS, linePattern) {
  return Object.freeze({
    id,
    ownerModule,
    sourceFile,
    reason,
    expiresOn,
    kinds: [...kinds],
    ...linePattern ? { linePattern } : {}
  });
}
var PERSISTENCE_EXEMPTIONS = Object.freeze([
  exemption(
    "lib-extract-zip-caller-destination",
    "lib/extract-zip.ts",
    "lib/extract-zip.ts",
    "Extracts zip entries into a destination directory explicitly selected by the caller (skill/plugin/character-card staging), outside any implied LINGXI_HOME store.",
    "2027-01-31"
  ),
  exemption(
    "data-epoch-durable-json-primitive",
    "shared/data-epoch.cjs",
    "shared/data-epoch.cjs",
    "The same-directory temp write, fsync, rename, and temp cleanup primitive has no independent store identity; its durableWriteJson call sites are assigned to stamp or journal ownership.",
    "2026-10-31",
    ["mkdir", "write-file", "rename", "remove-path"],
    "(?:path[.]dirname\\(filePath\\)|serialized|temporaryPath)"
  ),
  exemption(
    "desktop-gpu-json-primitive",
    "desktop/src/shared/gpu-startup-policy.cjs",
    "desktop/src/shared/gpu-startup-policy.cjs",
    "Generic atomic JSON primitive has no independent store ownership; its writeJson call sites are assigned to the GPU state or preferences owner.",
    "2026-10-31",
    ["mkdir", "write-file", "rename"],
    "(?:path[.]dirname\\(filePath\\)|tmpPath, JSON|stringify|tmpPath, filePath)"
  ),
  exemption(
    "desktop-external-text-file",
    "desktop/file-text-io.cjs",
    "desktop/file-text-io.cjs",
    "Writes an absolute artifact/editor path explicitly selected by the caller, outside any implied LINGXI_HOME store.",
    "2027-01-31"
  ),
  exemption(
    "desktop-legacy-update-path-cleanup",
    "desktop/auto-updater.cjs",
    "desktop/auto-updater.cjs",
    "Deletes the exact legacy last-update-version source and its now-empty legacy directory outside the active LINGXI_HOME.",
    "2026-10-31",
    ["remove-path"],
    "(?:wrongFile|wrongDir)"
  ),
  exemption(
    "desktop-electron-update-cache",
    "desktop/auto-updater.cjs",
    "desktop/auto-updater.cjs",
    "Deletes Electron updater's regenerable pending cache under app.getPath(userData), not Hana application state.",
    "2027-01-31",
    ["remove-path"],
    "cacheDir"
  ),
  exemption(
    "desktop-screenshot-html-temp",
    "desktop/main.cjs",
    "desktop/main.cjs",
    "Writes and removes one request-scoped screenshot HTML file under the operating-system temporary directory.",
    "2027-01-31",
    ["write-file", "remove-path"],
    "tmpHtml"
  ),
  exemption(
    "desktop-skill-preview-temp",
    "desktop/main.cjs",
    "desktop/main.cjs",
    "Creates a request-scoped skill preview directory under Electron's operating-system temp path.",
    "2027-01-31",
    ["mkdir"],
    "tmpDir"
  ),
  exemption(
    "desktop-caller-selected-output",
    "desktop/main.cjs",
    "desktop/main.cjs",
    "Writes an absolute path selected by the renderer or a screenshot directory selected by the user; no LINGXI_HOME store is implied.",
    "2027-01-31",
    ["mkdir", "write-file", "copy-file"],
    "(?:filePath, content|path[.]dirname\\(resolved\\)|resolved, Buffer|path[.]dirname\\(destinationPath\\)|sourcePath, destinationPath|mkdirSync\\(dir|filePath, pngBuffer)"
  ),
  exemption(
    "desktop-observability-export-output",
    "desktop/main.cjs",
    "desktop/main.cjs",
    "Streams the Model Observatory NDJSON export into a user-selected save-dialog path and deletes the partial file when the renderer aborts; no LINGXI_HOME store is implied.",
    "2027-01-31",
    ["write-file", "remove-path"],
    "(?:result[.]filePath|sessionInfo[.]fd|sessionInfo[.]filePath)"
  ),
  exemption(
    "desktop-office-render-output",
    "desktop/src/office-pdf-helper.cjs",
    "desktop/src/office-pdf-helper.cjs",
    "Writes the output path supplied by a validated office render job; the owning plugin records the durable job/session state.",
    "2027-01-31"
  ),
  exemption(
    "workspace-skill-delete",
    "server/routes/desk.ts",
    "server/routes/desk.ts",
    "Deletes a skill directory only after the route proves it belongs to the explicitly mounted workspace skill catalog, outside Hana-owned persistence.",
    "2027-01-31",
    ["remove-path"],
    "skillDir"
  ),
  exemption(
    "in-memory-confirm-registry",
    "lib/confirm-store.ts",
    "server/index.ts",
    "ConfirmStore is deliberately process-memory-only: pending approvals time out or abort with their session and have no restart replay contract.",
    "2027-01-31",
    ["persistent-store-constructor"],
    "ConfirmStore"
  ),
  exemption(
    "compat-directory-seeding",
    "lib/compat/checks/dirs.ts",
    "lib/compat/checks/dirs.ts",
    "Compatibility check creates a fixed set of registered LINGXI_HOME roots; flow attribution remains with the first-run coordinator.",
    "2026-10-31"
  ),
  exemption(
    "cross-store-first-run-bootstrap",
    "core/first-run.ts",
    "core/first-run.ts",
    "First-run seeds several registered stores from packaged templates. Flow-sensitive attribution moves into the future coordinator; this exact composition file is not ignored by directory.",
    "2026-10-31"
  ),
  exemption(
    "conditional-upload-target",
    "server/routes/upload.ts",
    "server/routes/upload.ts",
    "resolveUploadTarget sends the same syntactic write sites to registered session-files storage when sessionPath is present or to the registered legacy uploads cache otherwise; flow-sensitive site attribution is required to distinguish them.",
    "2026-10-31",
    ["mkdir", "copy-file", "write-file"]
  ),
  exemption(
    "server-identity-atomic-helper",
    "core/server-identity.ts",
    "core/server-identity.ts",
    "The file-local atomic helper receives paths only from the separately registered server-node and user/studio registry call sites.",
    "2026-10-31",
    ["mkdir", "atomic-write"],
    "(?:path[.]dirname\\(filePath\\)|atomicWriteSync\\(filePath)"
  ),
  exemption(
    "local-user-account-atomic-helper",
    "core/local-user-account.ts",
    "core/local-user-account.ts",
    "The file-local atomic helper receives paths only from the separately registered users.json and local-user-auth.json call sites.",
    "2026-10-31",
    ["mkdir", "secret-write"],
    "(?:path[.]dirname\\(filePath\\)|writeSecretFileSync\\(filePath)"
  ),
  exemption(
    "character-card-copy-helper",
    "lib/character-cards/service.ts",
    "lib/character-cards/service.ts",
    "The file-local copy helper is flow-dependent: callers target either token-scoped card staging or an explicitly registered skill destination.",
    "2026-10-31",
    ["mkdir", "copy-file"],
    "(?:path[.]dirname\\(dst\\)|sourcePath, dst)"
  ),
  exemption(
    "external-desk-workspace-roots",
    "server/routes/desk.ts",
    "server/routes/desk.ts",
    "Creates an approved external workspace root or its .agents/skills directory; the mount registry owns authorization, not the external content.",
    "2027-01-31",
    ["mkdir"],
    "(?:skillsDir|fs[.]mkdirSync\\(dir|baseDir)"
  ),
  exemption(
    "external-beautify-markdown-output",
    "plugins/beautify/lib/markdown-cover-service.ts",
    "plugins/beautify/lib/markdown-cover-service.ts",
    "Copies a generated cover beside an explicitly selected workspace Markdown file and atomically updates that external file.",
    "2027-01-31"
  ),
  exemption(
    "external-mount-writes",
    "core/mount-aware-file-service.ts",
    "core/mount-aware-file-service.ts",
    "Creates user-selected workspace or mounted roots whose ownership is recorded by studio-mounts.json rather than by LINGXI_HOME path inventory.",
    "2027-01-31"
  ),
  exemption(
    "external-resource-io",
    "lib/resource-io/providers/local-fs-provider.ts",
    "lib/resource-io/providers/local-fs-provider.ts",
    "Writes explicit user-selected local filesystem resource targets outside Hana persistence ownership.",
    "2027-01-31"
  ),
  exemption(
    "external-file-ref-io",
    "lib/file-ref/resource-io.ts",
    "lib/file-ref/resource-io.ts",
    "Writes an explicit tool output path supplied by the caller; no LINGXI_HOME store is implied.",
    "2027-01-31"
  ),
  exemption(
    "external-default-workspace",
    "shared/default-workspace.ts",
    "shared/default-workspace.ts",
    "Creates the user-selected default workspace outside LINGXI_HOME.",
    "2027-01-31"
  ),
  exemption(
    "external-heartbeat-workspace",
    "lib/desk/heartbeat.ts",
    "lib/desk/heartbeat.ts",
    "Writes heartbeat/task files inside an explicitly mounted workspace, not a Hana-owned persistence root.",
    "2027-01-31"
  ),
  exemption(
    "windows-uia-request-temp",
    "core/computer-use/providers/windows-uia-provider.ts",
    "core/computer-use/providers/windows-uia-provider.ts",
    "Creates helper/request files in the operating-system temporary directory for one Windows UIA call.",
    "2027-01-31"
  ),
  exemption(
    "generic-safe-fs-primitives",
    "shared/safe-fs.ts",
    "shared/safe-fs.ts",
    "Shared atomic-write primitives have no store ownership; callers are scanned and assigned to concrete descriptors.",
    "2027-01-31"
  ),
  exemption(
    "generic-secret-fs-primitives",
    "shared/secret-fs.ts",
    "shared/secret-fs.ts",
    "Shared owner-only write primitives have no store ownership; callers are scanned and assigned to concrete descriptors.",
    "2027-01-31"
  ),
  exemption(
    "generic-archive-writer",
    "shared/artifact-core/ustar.cjs",
    "shared/artifact-core/ustar.cjs",
    "The archive codec writes caller-selected archive/extraction paths; signed artifact ownership is assigned at its callers.",
    "2027-01-31"
  ),
  exemption(
    "generic-zip-writer",
    "lib/zip-writer.ts",
    "lib/zip-writer.ts",
    "The ZIP codec writes a caller-selected destination and has no independent persistent store.",
    "2027-01-31"
  ),
  exemption(
    "sandbox-command-output",
    "lib/exec-command/runner.ts",
    "lib/exec-command/runner.ts",
    "Writes command output to an explicit tool-selected path under the active sandbox/workspace.",
    "2027-01-31"
  ),
  exemption(
    "sandbox-script-output",
    "lib/sandbox/script.ts",
    "lib/sandbox/script.ts",
    "Writes a generated script into a request-scoped sandbox or operating-system temporary directory.",
    "2027-01-31"
  ),
  exemption(
    "web-fetch-spill",
    "lib/tools/web-fetch.ts",
    "lib/tools/web-fetch.ts",
    "Writes the truncated web_fetch full text to an operating-system temporary spill file so the model can recover the remainder with the read tool; outside any implied LINGXI_HOME store.",
    "2027-01-31",
    ["write-file"]
  ),
  exemption(
    "sandbox-office-media-temp",
    "lib/sandbox/read-office-media.ts",
    "lib/sandbox/read-office-media.ts",
    "Writes extracted office media to a request-scoped temporary directory.",
    "2027-01-31"
  ),
  exemption(
    "sandbox-win32-command-temp",
    "lib/sandbox/win32-exec.ts",
    "lib/sandbox/win32-exec.ts",
    "Writes a request-scoped Windows command wrapper outside persistent Hana state.",
    "2027-01-31"
  ),
  exemption(
    "url-provider-download-temp",
    "lib/resource-io/providers/url-provider.ts",
    "lib/resource-io/providers/url-provider.ts",
    "Writes a bounded network download to the caller-selected resource destination.",
    "2027-01-31"
  ),
  exemption(
    "git-worktree-user-repo-parent",
    "server/git/git-command.ts",
    "server/git/git-command.ts",
    "Creates the worktrees parent directory inside the caller's own git repository layout right before `git worktree add`; a user-selected repository location, outside any implied LINGXI_HOME store.",
    "2027-01-31",
    ["mkdir"],
    "list[.]root"
  )
]);

// core/data-epoch-migrations.ts
var DATA_EPOCH_MIGRATIONS = Object.freeze([]);
var DATA_EPOCH_BREAKING_REVIEWS = Object.freeze([]);

// core/data-epoch-coordinator.ts
function stampState(stamp) {
  return `${stamp.minimumReaderEpoch}/${stamp.committedDataEpoch}`;
}
function transitionConsistency(journal, stampRead) {
  if (stampRead.status === "corrupt") return { valid: false, detail: `stamp is corrupt: ${stampRead.detail}` };
  const sourceSteady = stampRead.status === "ok" && stampRead.stamp.minimumReaderEpoch === journal.fromEpoch && stampRead.stamp.committedDataEpoch === journal.fromEpoch;
  const barrierRaised = stampRead.status === "ok" && stampRead.stamp.minimumReaderEpoch === journal.toEpoch && stampRead.stamp.committedDataEpoch === journal.fromEpoch;
  const targetCommitted = stampRead.status === "ok" && stampRead.stamp.minimumReaderEpoch === journal.toEpoch && stampRead.stamp.committedDataEpoch === journal.toEpoch;
  let valid = false;
  if (journal.phase === "prepared") valid = stampRead.status === "missing" || sourceSteady;
  else if (journal.phase === "checkpoint_complete") valid = stampRead.status === "missing" || sourceSteady || barrierRaised;
  else if (journal.phase === "committed") valid = targetCommitted;
  else if (journal.phase === "validated") valid = barrierRaised || targetCommitted;
  else valid = barrierRaised;
  if (!valid) {
    const actual = stampRead.status === "missing" ? "missing" : stampState(stampRead.stamp);
    return { valid: false, detail: `journal phase ${journal.phase} contradicts stamp state ${actual}` };
  }
  return { valid: true, targetCommitted };
}
function inspectDataEpochMaintenance(homeDir) {
  const journalRead = (0, import_data_epoch.readDataEpochJournal)(homeDir);
  if (journalRead.status === "corrupt") {
    return { status: "corrupt", reason: "corrupt-journal", detail: journalRead.detail };
  }
  const stampRead = (0, import_data_epoch.readDataEpochStamp)(homeDir);
  if (stampRead.status === "corrupt") {
    return { status: "corrupt", reason: "corrupt-stamp", detail: stampRead.detail };
  }
  if (journalRead.status === "missing") {
    if (stampRead.status === "ok" && stampRead.stamp.minimumReaderEpoch !== stampRead.stamp.committedDataEpoch) {
      return {
        status: "corrupt",
        reason: "inconsistent-transition-state",
        detail: "stamp has an uncommitted reader barrier but no transition journal"
      };
    }
    return { status: "none" };
  }
  const journal = journalRead.journal;
  const consistency = transitionConsistency(journal, stampRead);
  if ("detail" in consistency) {
    return { status: "corrupt", reason: "corrupt-transition", detail: consistency.detail };
  }
  const allResumable = Object.values(journal.recoveryModes).every((mode) => mode === "resume-idempotent");
  let continuation;
  if (consistency.targetCommitted) continuation = "finalize-committed-tail";
  else if (journal.phase === "validated") continuation = "commit-validated";
  else if (journal.phase === "prepared" || journal.phase === "checkpoint_complete") continuation = "continue-before-migration";
  else continuation = allResumable ? "resume-idempotent" : "restore-only";
  return {
    status: "incomplete",
    transitionId: journal.transitionId,
    fromEpoch: journal.fromEpoch,
    toEpoch: journal.toEpoch,
    phase: journal.phase,
    checkpointId: journal.checkpointId,
    affectedStoreIds: [...journal.affectedStoreIds],
    recoveryModes: { ...journal.recoveryModes },
    continuation
  };
}

// core/data-epoch-restore.ts
import crypto3 from "crypto";
import fs5 from "fs";
import path6 from "path";

// core/data-epoch-checkpoint-provider.ts
var import_data_epoch2 = __toESM(require_data_epoch(), 1);
import crypto2 from "crypto";
import fs4 from "fs";
import path4 from "path";
var DATA_EPOCH_CHECKPOINT_FORMAT_VERSION = 1;
var DATA_EPOCH_CHECKPOINTS_DIRNAME = "data-epoch-checkpoints";
var DATA_EPOCH_CHECKPOINT_MAX_TOTAL_BYTES = 2 * 1024 * 1024 * 1024;
function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
function errorCode(error) {
  return typeof error === "object" && error !== null && "code" in error ? String(error.code) : void 0;
}
function toPosixPath(value) {
  return value.split(path4.sep).join("/");
}
function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function timestamp(clock) {
  const value = clock ? clock() : /* @__PURE__ */ new Date();
  const result = value instanceof Date ? value.toISOString() : value;
  if (typeof result !== "string" || Number.isNaN(Date.parse(result))) {
    throw new Error("data epoch checkpoint provider clock returned an invalid timestamp");
  }
  return result;
}
async function pathExists(candidate) {
  try {
    await fs4.promises.access(candidate);
    return true;
  } catch (error) {
    if (errorCode(error) === "ENOENT") return false;
    throw error;
  }
}
function validatePatternShape(pattern) {
  if (typeof pattern !== "string" || pattern.length === 0) {
    throw new Error("store path pattern must be a non-empty string");
  }
  if (pattern.startsWith("/") || pattern.includes("\\") || /^[A-Za-z]:/.test(pattern)) {
    throw new Error(`store path pattern must be a relative POSIX path: ${pattern}`);
  }
  const segments = pattern.split("/");
  for (const segment of segments) {
    if (segment.length === 0) {
      throw new Error(`store path pattern has an empty path segment: ${pattern}`);
    }
    if (segment === "." || segment === "..") {
      throw new Error(`store path pattern contains a path traversal segment: ${pattern}`);
    }
  }
  return segments;
}
function compileSegmentMatcher(segment, fullPattern) {
  let hasPlaceholder = false;
  let regexSource = "";
  let literalRun = "";
  let index = 0;
  while (index < segment.length) {
    const ch = segment[index];
    if (ch === "{") {
      const close = segment.indexOf("}", index + 1);
      if (close === -1) {
        throw new Error(`store path pattern segment "${segment}" has an unmatched "{" (pattern: ${fullPattern})`);
      }
      const name = segment.slice(index + 1, close);
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
        throw new Error(`store path pattern segment "${segment}" has an invalid placeholder name "{${name}}" (pattern: ${fullPattern})`);
      }
      regexSource += `${escapeRegExp(literalRun)}([^/]+)`;
      literalRun = "";
      hasPlaceholder = true;
      index = close + 1;
      continue;
    }
    if (ch === "}") {
      throw new Error(`store path pattern segment "${segment}" has an unmatched "}" (pattern: ${fullPattern})`);
    }
    literalRun += ch;
    index += 1;
  }
  regexSource += escapeRegExp(literalRun);
  if (!hasPlaceholder) return { kind: "literal", value: segment };
  return { kind: "pattern", regex: new RegExp(`^${regexSource}$`) };
}
function statEntryKind(absPath) {
  let stat;
  try {
    stat = fs4.lstatSync(absPath);
  } catch (error) {
    if (errorCode(error) === "ENOENT") return null;
    throw error;
  }
  if (stat.isSymbolicLink()) {
    throw new Error(`data epoch checkpoint provider refuses to traverse a symbolic link: ${absPath}`);
  }
  if (stat.isDirectory()) return "dir";
  if (stat.isFile()) return "file";
  throw new Error(`data epoch checkpoint provider found an unsupported filesystem entry: ${absPath}`);
}
function listChildNames(dirPath, matcher) {
  if (matcher.kind === "literal") {
    try {
      fs4.lstatSync(path4.join(dirPath, matcher.value));
      return [matcher.value];
    } catch (error) {
      if (errorCode(error) === "ENOENT") return [];
      throw error;
    }
  }
  let entries;
  try {
    entries = fs4.readdirSync(dirPath, { withFileTypes: true });
  } catch (error) {
    const code = errorCode(error);
    if (code === "ENOENT" || code === "ENOTDIR") return [];
    throw error;
  }
  return entries.filter((entry) => matcher.regex.test(entry.name)).map((entry) => entry.name);
}
function expandStorePathPattern(baseDir, pattern) {
  const segments = validatePatternShape(pattern);
  const matchers = segments.map((segment) => compileSegmentMatcher(segment, pattern));
  let level = [{ absPath: baseDir, relParts: [] }];
  matchers.forEach((matcher, index) => {
    const isLast = index === matchers.length - 1;
    const nextLevel = [];
    for (const node of level) {
      for (const name of listChildNames(node.absPath, matcher)) {
        const absPath = path4.join(node.absPath, name);
        const kind = statEntryKind(absPath);
        if (kind === null) continue;
        if (!isLast && kind !== "dir") continue;
        nextLevel.push({ absPath, relParts: [...node.relParts, name] });
      }
    }
    level = nextLevel;
  });
  return level.map((node) => {
    const kind = statEntryKind(node.absPath);
    if (kind === null) {
      throw new Error(`store path pattern match disappeared while expanding "${pattern}": ${node.absPath}`);
    }
    return { absPath: node.absPath, relPath: node.relParts.join("/"), isDirectory: kind === "dir" };
  }).sort((left, right) => left.relPath.localeCompare(right.relPath));
}
function hashAndSizeFile(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto2.createHash("sha256");
    let bytes = 0;
    const stream = fs4.createReadStream(filePath);
    stream.on("error", reject);
    stream.on("data", (chunk) => {
      bytes += chunk.length;
      hash.update(chunk);
    });
    stream.on("end", () => resolve({ bytes, sha256: hash.digest("hex") }));
  });
}
async function captureFileCopy(srcPath, destPath) {
  await fs4.promises.mkdir(path4.dirname(destPath), { recursive: true });
  await fs4.promises.copyFile(srcPath, destPath);
}
async function captureSqliteBackup(srcPath, destPath) {
  await fs4.promises.mkdir(path4.dirname(destPath), { recursive: true });
  const { default: Database } = await Promise.resolve().then(() => __toESM(require_lib(), 1));
  const db = new Database(srcPath, { readonly: true, fileMustExist: true });
  try {
    await db.backup(destPath);
  } finally {
    db.close();
  }
}
function walkFilesRecursive(dirPath) {
  const result = [];
  const stack = [dirPath];
  while (stack.length > 0) {
    const current = stack.pop();
    const entries = fs4.readdirSync(current, { withFileTypes: true });
    for (const entry of entries) {
      const absPath = path4.join(current, entry.name);
      if (entry.isSymbolicLink()) {
        throw new Error(`data epoch checkpoint provider refuses to traverse a symbolic link: ${absPath}`);
      }
      if (entry.isDirectory()) {
        stack.push(absPath);
      } else if (entry.isFile()) {
        result.push(absPath);
      } else {
        throw new Error(`data epoch checkpoint provider found an unsupported filesystem entry: ${absPath}`);
      }
    }
  }
  return result.sort();
}
function isSqliteSidecarPattern(pattern) {
  return pattern.endsWith("-wal") || pattern.endsWith("-shm");
}
async function captureStoreItems(homeDir, tmpDir, descriptor) {
  const isSqlite = descriptor.format === "sqlite";
  const patterns = isSqlite ? descriptor.pathPatterns.filter((pattern) => !isSqliteSidecarPattern(pattern)) : descriptor.pathPatterns;
  const items = [];
  for (const pattern of patterns) {
    const matches = expandStorePathPattern(homeDir, pattern);
    for (const match of matches) {
      if (match.isDirectory && descriptor.pathKind !== "tree") {
        throw new Error(
          `data epoch checkpoint provider found a directory where store "${descriptor.id}" declares pathKind "file": ${match.relPath}`
        );
      }
      if (!match.isDirectory && descriptor.pathKind === "tree") {
        throw new Error(
          `data epoch checkpoint provider found a file where store "${descriptor.id}" declares pathKind "tree": ${match.relPath}`
        );
      }
      if (match.isDirectory) {
        for (const absFile of walkFilesRecursive(match.absPath)) {
          const relPath = toPosixPath(path4.relative(homeDir, absFile));
          const destPath = path4.join(tmpDir, "stores", descriptor.id, ...relPath.split("/"));
          await captureFileCopy(absFile, destPath);
          const { bytes, sha256 } = await hashAndSizeFile(destPath);
          items.push({ storeId: descriptor.id, relPath, bytes, sha256 });
        }
      } else {
        const relPath = toPosixPath(path4.relative(homeDir, match.absPath));
        const destPath = path4.join(tmpDir, "stores", descriptor.id, ...relPath.split("/"));
        if (isSqlite) {
          await captureSqliteBackup(match.absPath, destPath);
        } else {
          await captureFileCopy(match.absPath, destPath);
        }
        const { bytes, sha256 } = await hashAndSizeFile(destPath);
        items.push({ storeId: descriptor.id, relPath, bytes, sha256 });
      }
    }
  }
  return items;
}
async function readCheckpointMetadata(dir) {
  const metadataPath = path4.join(dir, "metadata.json");
  let raw;
  try {
    raw = await fs4.promises.readFile(metadataPath, "utf8");
  } catch (error) {
    throw new Error(`data epoch checkpoint ${dir} is missing metadata.json: ${errorMessage(error)}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new Error(`data epoch checkpoint ${dir} has an unparsable metadata.json: ${errorMessage(error)}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`data epoch checkpoint ${dir} metadata.json must be a JSON object`);
  }
  const value = parsed;
  if (value.formatVersion !== DATA_EPOCH_CHECKPOINT_FORMAT_VERSION) {
    throw new Error(`data epoch checkpoint ${dir} has an unsupported metadata formatVersion: ${String(value.formatVersion)}`);
  }
  if (value.complete !== true) {
    throw new Error(`data epoch checkpoint ${dir} metadata.json is not marked complete`);
  }
  if (typeof value.transitionId !== "string" || value.transitionId.length === 0) {
    throw new Error(`data epoch checkpoint ${dir} metadata.json is missing transitionId`);
  }
  if (!Array.isArray(value.items)) {
    throw new Error(`data epoch checkpoint ${dir} metadata.json is missing an items array`);
  }
  for (const item of value.items) {
    if (!item || typeof item.storeId !== "string" || typeof item.relPath !== "string" || typeof item.bytes !== "number" || typeof item.sha256 !== "string") {
      throw new Error(`data epoch checkpoint ${dir} metadata.json has a malformed item entry`);
    }
  }
  return value;
}
async function verifyCheckpointDir(dir, expectedId) {
  const metadata = await readCheckpointMetadata(dir);
  if (expectedId !== void 0 && metadata.transitionId !== expectedId) {
    throw new Error(
      `data epoch checkpoint ${dir} metadata transitionId "${metadata.transitionId}" does not match expected "${expectedId}"`
    );
  }
  for (const item of metadata.items) {
    const itemPath = path4.join(dir, "stores", item.storeId, ...item.relPath.split("/"));
    let stat;
    try {
      stat = await fs4.promises.stat(itemPath);
    } catch (error) {
      throw new Error(
        `data epoch checkpoint ${dir} is missing the captured file for storeId "${item.storeId}" relPath "${item.relPath}": ${errorMessage(error)}`
      );
    }
    if (stat.size !== item.bytes) {
      throw new Error(
        `data epoch checkpoint ${dir} byte-count mismatch for storeId "${item.storeId}" relPath "${item.relPath}": expected ${item.bytes}, found ${stat.size}`
      );
    }
    const { sha256 } = await hashAndSizeFile(itemPath);
    if (sha256 !== item.sha256) {
      throw new Error(
        `data epoch checkpoint ${dir} sha256 mismatch for storeId "${item.storeId}" relPath "${item.relPath}": expected ${item.sha256}, found ${sha256}`
      );
    }
  }
  return metadata;
}
function receiptFromMetadata(dir, metadata) {
  const totalBytes = metadata.items.reduce((sum, item) => sum + item.bytes, 0);
  return { id: metadata.transitionId, dir, itemCount: metadata.items.length, totalBytes };
}
async function cleanupStaleTmpSiblings(checkpointsRoot, transitionId) {
  let entries;
  try {
    entries = await fs4.promises.readdir(checkpointsRoot, { withFileTypes: true });
  } catch (error) {
    if (errorCode(error) === "ENOENT") return;
    throw error;
  }
  const prefix = `${transitionId}.tmp-`;
  for (const entry of entries) {
    if (entry.isDirectory() && entry.name.startsWith(prefix)) {
      await fs4.promises.rm(path4.join(checkpointsRoot, entry.name), { recursive: true, force: true });
    }
  }
}
function createDataEpochCheckpointProvider(options = {}) {
  const stores = options.stores ?? PERSISTENT_STORES;
  const storesById = new Map(stores.map((store) => [store.id, store]));
  const clock = options.clock;
  async function create(input) {
    const { homeDir, fromEpoch, toEpoch, transitionId, affectedStoreIds } = input;
    if (typeof transitionId !== "string" || transitionId.length === 0) {
      throw new Error("data epoch checkpoint provider create() requires a non-empty transitionId");
    }
    const checkpointsRoot = path4.join(homeDir, DATA_EPOCH_CHECKPOINTS_DIRNAME);
    const publishedDir = path4.join(checkpointsRoot, transitionId);
    await cleanupStaleTmpSiblings(checkpointsRoot, transitionId);
    if (await pathExists(publishedDir)) {
      try {
        const metadata2 = await verifyCheckpointDir(publishedDir, transitionId);
        return receiptFromMetadata(publishedDir, metadata2);
      } catch {
        const invalidDir = `${publishedDir}.invalid-${Date.now()}`;
        await fs4.promises.rename(publishedDir, invalidDir);
      }
    }
    const descriptors = affectedStoreIds.map((id) => {
      const descriptor = storesById.get(id);
      if (!descriptor) {
        throw new Error(`data epoch checkpoint provider: unknown store id "${id}"`);
      }
      return descriptor;
    });
    const tmpDir = `${publishedDir}.tmp-${process.pid}-${crypto2.randomBytes(6).toString("hex")}`;
    let metadata;
    try {
      await fs4.promises.mkdir(tmpDir, { recursive: true });
      const items = [];
      for (const descriptor of descriptors) {
        items.push(...await captureStoreItems(homeDir, tmpDir, descriptor));
      }
      metadata = {
        formatVersion: DATA_EPOCH_CHECKPOINT_FORMAT_VERSION,
        transitionId,
        fromEpoch,
        toEpoch,
        affectedStoreIds: [...affectedStoreIds],
        createdAt: timestamp(clock),
        items,
        complete: true
      };
      await fs4.promises.writeFile(path4.join(tmpDir, "metadata.json"), `${JSON.stringify(metadata, null, 2)}
`, "utf8");
      await fs4.promises.rename(tmpDir, publishedDir);
    } catch (error) {
      await fs4.promises.rm(tmpDir, { recursive: true, force: true }).catch(() => {
      });
      throw error;
    }
    return receiptFromMetadata(publishedDir, metadata);
  }
  async function verify(checkpoint) {
    const dir = checkpoint.dir;
    if (typeof dir !== "string" || dir.length === 0) {
      throw new Error('data epoch checkpoint provider verify() requires a receipt with a string "dir"');
    }
    await verifyCheckpointDir(dir, checkpoint.id);
  }
  return { create, verify };
}

// core/data-epoch-restore.ts
var import_server_info_probe2 = __toESM(require_server_info_probe(), 1);
var import_data_epoch3 = __toESM(require_data_epoch(), 1);

// shared/hana-root.ts
import path5 from "path";
import { fileURLToPath as fileURLToPath2 } from "url";
var __dirname = path5.dirname(fileURLToPath2(import.meta.url));
var LINGXI_ROOT = process.env.LINGXI_ROOT || path5.resolve(__dirname, "..");
function fromRoot(...segments) {
  return path5.join(LINGXI_ROOT, ...segments);
}

// core/data-epoch-restore.ts
var DATA_EPOCH_RESTORE_QUARANTINE_DIRNAME = "data-epoch-restore-quarantine";
var DATA_EPOCH_RESTORE_LOG_FILENAME = "data-epoch-restores.log";
var DATA_EPOCH_RESTORE_RECEIPT_FILENAME = "restore-receipt.json";
function errorMessage2(error) {
  return error instanceof Error ? error.message : String(error);
}
function errorCode2(error) {
  return typeof error === "object" && error !== null && "code" in error ? String(error.code) : void 0;
}
function toPosixPath2(value) {
  return value.split(path6.sep).join("/");
}
function timestamp2(clock) {
  const value = clock ? clock() : /* @__PURE__ */ new Date();
  const result = value instanceof Date ? value.toISOString() : value;
  if (typeof result !== "string" || Number.isNaN(Date.parse(result))) {
    throw new Error("data epoch restore clock returned an invalid timestamp");
  }
  return result;
}
async function pathExists2(candidate) {
  try {
    await fs5.promises.access(candidate);
    return true;
  } catch (error) {
    if (errorCode2(error) === "ENOENT") return false;
    throw error;
  }
}
async function notifyFault(hook, event) {
  if (hook) await hook(event);
}
async function resolvePackageVersion() {
  const raw = await fs5.promises.readFile(fromRoot("package.json"), "utf8");
  const pkg = JSON.parse(raw);
  if (typeof pkg.version !== "string" || pkg.version.length === 0) {
    throw new Error("data epoch restore could not resolve the running kernel's package version");
  }
  return pkg.version;
}
async function assertNoLiveServer(homeDir) {
  const serverInfoPath = path6.join(homeDir, "server-info.json");
  let info = null;
  try {
    info = JSON.parse(await fs5.promises.readFile(serverInfoPath, "utf8"));
  } catch {
    return;
  }
  const probe = await (0, import_server_info_probe2.probeServerInfo)({ info });
  if ((0, import_server_info_probe2.isForeignServerBlocking)(probe.status)) {
    const detail = (0, import_server_info_probe2.describeForeignServerBlock)({ status: probe.status, info }) ?? `a kernel is currently responding for this data directory (probe status: ${probe.status})`;
    throw new Error(`restoreDataEpochCheckpoint refuses to run while a kernel is live for this home:
${detail}`);
  }
}
function walkFilesRecursive2(dirPath) {
  const result = [];
  const stack = [dirPath];
  while (stack.length > 0) {
    const current = stack.pop();
    const entries = fs5.readdirSync(current, { withFileTypes: true });
    for (const entry of entries) {
      const absPath = path6.join(current, entry.name);
      if (entry.isSymbolicLink()) {
        throw new Error(`data epoch restore refuses to traverse a symbolic link: ${absPath}`);
      }
      if (entry.isDirectory()) {
        stack.push(absPath);
      } else if (entry.isFile()) {
        result.push(absPath);
      } else {
        throw new Error(`data epoch restore found an unsupported filesystem entry: ${absPath}`);
      }
    }
  }
  return result.sort();
}
function collectActualStoreFiles(homeDir, descriptor) {
  const results = [];
  for (const pattern of descriptor.pathPatterns) {
    const matches = expandStorePathPattern(homeDir, pattern);
    for (const match of matches) {
      if (match.isDirectory) {
        if (descriptor.pathKind !== "tree") {
          throw new Error(
            `data epoch restore found a directory where store "${descriptor.id}" declares pathKind "file": ${match.relPath}`
          );
        }
        for (const absFile of walkFilesRecursive2(match.absPath)) {
          results.push({ relPath: toPosixPath2(path6.relative(homeDir, absFile)), absPath: absFile });
        }
      } else {
        if (descriptor.pathKind === "tree") {
          throw new Error(
            `data epoch restore found a file where store "${descriptor.id}" declares pathKind "tree": ${match.relPath}`
          );
        }
        results.push({ relPath: toPosixPath2(path6.relative(homeDir, match.absPath)), absPath: match.absPath });
      }
    }
  }
  return results;
}
function sha256File(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto3.createHash("sha256");
    let bytes = 0;
    const stream = fs5.createReadStream(filePath);
    stream.on("error", reject);
    stream.on("data", (chunk) => {
      bytes += chunk.length;
      hash.update(chunk);
    });
    stream.on("end", () => resolve({ bytes, sha256: hash.digest("hex") }));
  });
}
async function quarantineDestination(quarantineStoreDir, relPath) {
  const segments = relPath.split("/");
  const base = path6.join(quarantineStoreDir, ...segments);
  if (!await pathExists2(base)) return base;
  const dir = path6.dirname(base);
  const ext = path6.extname(base);
  const stem = path6.basename(base, ext);
  for (let attempt = 2; ; attempt += 1) {
    const candidate = path6.join(dir, `${stem}.dup-${attempt}${ext}`);
    if (!await pathExists2(candidate)) return candidate;
  }
}
async function restoreOneStore(args) {
  const { homeDir, descriptor, capturedItems, checkpointDir, quarantineRoot, faultHook } = args;
  const capturedByRelPath = new Map(capturedItems.map((item) => [item.relPath, item]));
  const existing = collectActualStoreFiles(homeDir, descriptor);
  const alreadyRestored = existing.length === capturedItems.length && (await Promise.all(existing.map(async (file) => {
    const captured = capturedByRelPath.get(file.relPath);
    if (!captured) return false;
    const stat = await fs5.promises.stat(file.absPath).catch(() => null);
    return stat != null && stat.size === captured.bytes;
  }))).every(Boolean);
  if (!alreadyRestored) {
    const quarantineStoreDir = path6.join(quarantineRoot, descriptor.id);
    for (const file of existing) {
      const destination = await quarantineDestination(quarantineStoreDir, file.relPath);
      await fs5.promises.mkdir(path6.dirname(destination), { recursive: true });
      await fs5.promises.rename(file.absPath, destination);
    }
    await notifyFault(faultHook, `restore:store-quarantined:${descriptor.id}`);
    for (const item of capturedItems) {
      const source = path6.join(checkpointDir, "stores", descriptor.id, ...item.relPath.split("/"));
      const destination = path6.join(homeDir, ...item.relPath.split("/"));
      await fs5.promises.mkdir(path6.dirname(destination), { recursive: true });
      await fs5.promises.copyFile(source, destination);
    }
    await notifyFault(faultHook, `restore:store-copied-back:${descriptor.id}`);
  }
  const reconciled = collectActualStoreFiles(homeDir, descriptor);
  if (reconciled.length !== capturedItems.length) {
    throw new Error(
      `data epoch restore reconciliation failed for store "${descriptor.id}": expected ${capturedItems.length} file(s) matching the checkpoint manifest, found ${reconciled.length}`
    );
  }
  const reconciledByRelPath = new Map(reconciled.map((file) => [file.relPath, file]));
  for (const item of capturedItems) {
    const file = reconciledByRelPath.get(item.relPath);
    if (!file) {
      throw new Error(`data epoch restore reconciliation failed for store "${descriptor.id}": missing ${item.relPath}`);
    }
    const { bytes, sha256 } = await sha256File(file.absPath);
    if (bytes !== item.bytes || sha256 !== item.sha256) {
      throw new Error(
        `data epoch restore reconciliation failed for store "${descriptor.id}" file "${item.relPath}": expected ${item.bytes}b/${item.sha256}, found ${bytes}b/${sha256}`
      );
    }
  }
}
async function readCheckpointMetadataForRestore(checkpointDir, transitionId) {
  const metadataPath = path6.join(checkpointDir, "metadata.json");
  const raw = await fs5.promises.readFile(metadataPath, "utf8");
  const value = JSON.parse(raw);
  if (value.transitionId !== transitionId) {
    throw new Error(
      `data epoch restore: checkpoint metadata transitionId "${value.transitionId}" does not match requested "${transitionId}"`
    );
  }
  return value;
}
async function restoreDataEpochCheckpoint(args) {
  const {
    homeDir,
    transitionId,
    confirmToken,
    log = console,
    clock,
    stores = PERSISTENT_STORES,
    faultHook
  } = args;
  if (typeof homeDir !== "string" || homeDir.length === 0) {
    throw new Error("restoreDataEpochCheckpoint requires a non-empty homeDir");
  }
  if (typeof transitionId !== "string" || transitionId.length === 0) {
    throw new Error("restoreDataEpochCheckpoint requires a non-empty transitionId");
  }
  const expectedToken = `restore ${transitionId}`;
  if (confirmToken !== expectedToken) {
    throw new Error(`restoreDataEpochCheckpoint requires the exact confirmation phrase "${expectedToken}"`);
  }
  await assertNoLiveServer(homeDir);
  const checkpointDir = path6.join(homeDir, DATA_EPOCH_CHECKPOINTS_DIRNAME, transitionId);
  const provider = createDataEpochCheckpointProvider({ stores });
  try {
    await provider.verify({ id: transitionId, dir: checkpointDir });
  } catch (error) {
    throw new Error(
      `restoreDataEpochCheckpoint: checkpoint verification failed for transitionId "${transitionId}": ${errorMessage2(error)}`
    );
  }
  const metadata = await readCheckpointMetadataForRestore(checkpointDir, transitionId);
  const { fromEpoch, toEpoch, affectedStoreIds } = metadata;
  const descriptorsById = new Map(stores.map((store) => [store.id, store]));
  for (const storeId of affectedStoreIds) {
    if (!descriptorsById.has(storeId)) {
      throw new Error(`restoreDataEpochCheckpoint: checkpoint references unknown store id "${storeId}"`);
    }
  }
  const restoreRead = (0, import_data_epoch3.readDataEpochRestoreJournal)(homeDir);
  let restoreId;
  let resumeFromPhase = null;
  if (restoreRead.status === "ok") {
    if (restoreRead.journal.transitionId !== transitionId) {
      throw new Error(
        `restoreDataEpochCheckpoint: a restore is already in progress for a different transitionId (${restoreRead.journal.transitionId}); refusing to start a restore for ${transitionId}`
      );
    }
    if (restoreRead.journal.fromEpoch !== fromEpoch) {
      throw new Error(
        `restoreDataEpochCheckpoint: the in-progress restore journal targets fromEpoch=${restoreRead.journal.fromEpoch}, but the checkpoint metadata for "${transitionId}" records fromEpoch=${fromEpoch}`
      );
    }
    restoreId = restoreRead.journal.restoreId;
    resumeFromPhase = restoreRead.journal.phase;
    log.warn(
      `[data-epoch-restore] resuming an interrupted restore for transitionId=${transitionId} (restoreId=${restoreId}) from phase ${resumeFromPhase}`
    );
  } else {
    const forwardRead = (0, import_data_epoch3.readDataEpochJournal)(homeDir);
    if (forwardRead.status === "ok") {
      if (forwardRead.journal.transitionId !== transitionId) {
        throw new Error(
          `restoreDataEpochCheckpoint: an in-progress forward transition journal exists for a different transitionId (${forwardRead.journal.transitionId}); refusing to restore ${transitionId} over it`
        );
      }
    } else if (forwardRead.status === "corrupt") {
      throw new Error(
        `restoreDataEpochCheckpoint refuses to run while the on-disk transition journal is unreadable: ${forwardRead.detail}`
      );
    }
    restoreId = crypto3.randomUUID();
  }
  const quarantineRoot = path6.join(homeDir, DATA_EPOCH_RESTORE_QUARANTINE_DIRNAME, restoreId);
  const receiptPath = path6.join(quarantineRoot, DATA_EPOCH_RESTORE_RECEIPT_FILENAME);
  const logPath = path6.join(homeDir, DATA_EPOCH_RESTORE_LOG_FILENAME);
  if (resumeFromPhase === null) {
    await (0, import_data_epoch3.writeDataEpochRestoreJournal)(homeDir, {
      restoreId,
      transitionId,
      fromEpoch,
      phase: "restore:starting",
      updatedAt: timestamp2(clock)
    });
    resumeFromPhase = "restore:starting";
    await notifyFault(faultHook, "restore:journal-written");
  }
  if (resumeFromPhase === "restore:starting") {
    for (const storeId of affectedStoreIds) {
      const descriptor = descriptorsById.get(storeId);
      const capturedItems = metadata.items.filter((item) => item.storeId === storeId);
      await restoreOneStore({ homeDir, descriptor, capturedItems, checkpointDir, quarantineRoot, faultHook });
    }
    await (0, import_data_epoch3.writeDataEpochRestoreJournal)(homeDir, {
      restoreId,
      transitionId,
      fromEpoch,
      phase: "restore:stores_restored",
      updatedAt: timestamp2(clock)
    });
    resumeFromPhase = "restore:stores_restored";
    await notifyFault(faultHook, "restore:stores-restored");
  }
  if (resumeFromPhase === "restore:stores_restored") {
    const lastVersion = await resolvePackageVersion();
    await (0, import_data_epoch3.republishDataEpochStampForRestore)({ homeDir, fromEpoch, lastVersion, updatedAt: timestamp2(clock) });
    await (0, import_data_epoch3.writeDataEpochRestoreJournal)(homeDir, {
      restoreId,
      transitionId,
      fromEpoch,
      phase: "restore:metadata_republished",
      updatedAt: timestamp2(clock)
    });
    resumeFromPhase = "restore:metadata_republished";
    await notifyFault(faultHook, "restore:metadata-republished");
  }
  if (!await pathExists2(receiptPath)) {
    const receipt = {
      schemaVersion: 1,
      restoreId,
      transitionId,
      fromEpoch,
      toEpoch,
      affectedStoreIds: [...affectedStoreIds],
      itemCount: metadata.items.length,
      totalBytes: metadata.items.reduce((sum, item) => sum + item.bytes, 0),
      checkpointDir,
      quarantineDir: quarantineRoot,
      restoredAt: timestamp2(clock)
    };
    await fs5.promises.mkdir(path6.dirname(logPath), { recursive: true });
    await fs5.promises.appendFile(logPath, `${JSON.stringify(receipt)}
`, "utf8");
    await (0, import_data_epoch3.durableWriteJson)(receiptPath, receipt);
  }
  await (0, import_data_epoch3.removeDataEpochJournal)(homeDir);
  return {
    restoreId,
    transitionId,
    fromEpoch,
    toEpoch,
    affectedStoreIds: [...affectedStoreIds],
    quarantineDir: quarantineRoot,
    receiptPath
  };
}

// cli/data.ts
var import_data_epoch4 = __toESM(require_data_epoch(), 1);
var import_contract_versions2 = __toESM(require_contract_versions2(), 1);
function checkpointsRootFor(lingxiHome) {
  return path7.join(lingxiHome, DATA_EPOCH_CHECKPOINTS_DIRNAME);
}
function listDataEpochCheckpoints(lingxiHome) {
  const root = checkpointsRootFor(lingxiHome);
  let entries;
  try {
    entries = fs6.readdirSync(root, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") return { checkpoints: [], skipped: 0 };
    throw error;
  }
  const checkpoints = [];
  let skipped = 0;
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    if (entry.name.includes(".tmp-") || entry.name.includes(".invalid-")) {
      skipped += 1;
      continue;
    }
    try {
      const raw = fs6.readFileSync(path7.join(root, entry.name, "metadata.json"), "utf-8");
      const metadata = JSON.parse(raw);
      if (!metadata || metadata.complete !== true || !Array.isArray(metadata.items)) {
        skipped += 1;
        continue;
      }
      const storeIds = new Set(metadata.items.map((item) => item.storeId));
      const totalBytes = metadata.items.reduce((sum, item) => sum + (item.bytes || 0), 0);
      checkpoints.push({
        transitionId: String(metadata.transitionId ?? entry.name),
        fromEpoch: Number(metadata.fromEpoch),
        toEpoch: Number(metadata.toEpoch),
        createdAt: String(metadata.createdAt ?? "unknown"),
        storeCount: storeIds.size,
        totalBytes
      });
    } catch {
      skipped += 1;
    }
  }
  checkpoints.sort((a, b) => a.createdAt < b.createdAt ? 1 : a.createdAt > b.createdAt ? -1 : 0);
  return { checkpoints, skipped };
}
function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}
async function runDataDiagnose({ lingxiHome = resolveCliLingxiHome() } = {}) {
  console.log(`Data-epoch diagnostics ${ansi.dim}(${lingxiHome})${ansi.reset}`);
  console.log(`  Kernel DATA_EPOCH    ${import_contract_versions2.DATA_EPOCH}`);
  const stampRead = (0, import_data_epoch4.readDataEpochStamp)(lingxiHome);
  if (stampRead.status === "ok") {
    const stamp = stampRead.stamp;
    console.log(`  Stamp                minimumReaderEpoch=${stamp.minimumReaderEpoch} committedDataEpoch=${stamp.committedDataEpoch}`);
    console.log(`  Last written by      ${stamp.lastVersion ?? "unknown"} at ${stamp.updatedAt ?? "unknown"}`);
  } else if (stampRead.status === "missing") {
    console.log(`  Stamp                ${ansi.dim}none (unstamped home)${ansi.reset}`);
  } else {
    console.log(`  Stamp                ${ansi.red}corrupt: ${stampRead.detail}${ansi.reset}`);
  }
  const forwardJournal = (0, import_data_epoch4.readDataEpochJournal)(lingxiHome);
  const restoreJournal = (0, import_data_epoch4.readDataEpochRestoreJournal)(lingxiHome);
  if (forwardJournal.status === "ok") {
    const j = forwardJournal.journal;
    console.log(`  Transition journal   ${j.transitionId} phase=${j.phase} ${j.fromEpoch}\u2192${j.toEpoch}`);
  } else if (restoreJournal.status === "ok") {
    const j = restoreJournal.journal;
    console.log(`  Restore journal      ${j.restoreId} (transitionId=${j.transitionId}) phase=${j.phase} fromEpoch=${j.fromEpoch}`);
  } else if (forwardJournal.status === "missing") {
    console.log(`  Journal              ${ansi.dim}none${ansi.reset}`);
  } else {
    console.log(`  Journal              ${ansi.red}corrupt: ${forwardJournal.detail}${ansi.reset}`);
  }
  const maintenance = inspectDataEpochMaintenance(lingxiHome);
  if (maintenance.status === "none") {
    console.log(`  Maintenance          ${ansi.green}steady, no transition in progress${ansi.reset}`);
  } else if (maintenance.status === "corrupt") {
    console.log(`  Maintenance          ${ansi.red}corrupt (${maintenance.reason}): ${maintenance.detail}${ansi.reset}`);
  } else {
    console.log(`  Maintenance          ${ansi.yellow}incomplete transition ${maintenance.transitionId} (${maintenance.fromEpoch}\u2192${maintenance.toEpoch}), phase=${maintenance.phase}${ansi.reset}`);
    console.log(`  Continuation         ${maintenance.continuation}`);
    console.log(`  Affected stores      ${maintenance.affectedStoreIds.join(", ") || "none"}`);
  }
  const { checkpoints, skipped } = listDataEpochCheckpoints(lingxiHome);
  const skippedNote = skipped ? ` (${skipped} incomplete/invalid, not listed)` : "";
  if (checkpoints.length === 0) {
    console.log(`  Checkpoints          ${ansi.dim}none available${ansi.reset}${skippedNote}`);
  } else {
    console.log(`  Checkpoints          ${checkpoints.length} available${skippedNote} \u2014 run \`hana data checkpoints\` for details`);
  }
  return 0;
}
async function runDataCheckpoints({ lingxiHome = resolveCliLingxiHome() } = {}) {
  const { checkpoints, skipped } = listDataEpochCheckpoints(lingxiHome);
  if (checkpoints.length === 0) {
    console.log(`${ansi.dim}No data-epoch checkpoints available.${ansi.reset}`);
    if (skipped) console.log(`${ansi.dim}(${skipped} incomplete/invalid checkpoint director${skipped === 1 ? "y" : "ies"} found and skipped.)${ansi.reset}`);
    return 0;
  }
  console.log(`Data-epoch checkpoints ${ansi.dim}(${lingxiHome})${ansi.reset}`);
  for (const checkpoint of checkpoints) {
    console.log("");
    console.log(`  ${ansi.bold}${checkpoint.transitionId}${ansi.reset}`);
    console.log(`    ${ansi.dim}Epoch${ansi.reset}    ${checkpoint.fromEpoch} \u2192 ${checkpoint.toEpoch}`);
    console.log(`    ${ansi.dim}Created${ansi.reset}  ${checkpoint.createdAt}`);
    console.log(`    ${ansi.dim}Stores${ansi.reset}   ${checkpoint.storeCount}`);
    console.log(`    ${ansi.dim}Size${ansi.reset}     ${formatBytes(checkpoint.totalBytes)}`);
  }
  if (skipped) {
    console.log("");
    console.log(`${ansi.dim}(${skipped} incomplete/invalid checkpoint director${skipped === 1 ? "y" : "ies"} found and skipped.)${ansi.reset}`);
  }
  return 0;
}
function defaultPromptConfirmation(question) {
  return new Promise((resolve) => {
    const rl = readline2.createInterface({ input: process.stdin, output: process.stdout });
    rl.question(question, (answer) => {
      rl.close();
      resolve(answer);
    });
  });
}
async function runDataRestore({
  transitionId,
  confirmToken = null,
  lingxiHome = resolveCliLingxiHome(),
  restore = restoreDataEpochCheckpoint,
  isTTY = process.stdin.isTTY === true,
  promptConfirmation = defaultPromptConfirmation
}) {
  if (!transitionId) {
    console.error(`${ansi.red}data restore requires a transitionId: hana data restore <transitionId>${ansi.reset}`);
    return 1;
  }
  const checkpointDir = path7.join(checkpointsRootFor(lingxiHome), transitionId);
  let metadata;
  try {
    metadata = JSON.parse(fs6.readFileSync(path7.join(checkpointDir, "metadata.json"), "utf-8"));
  } catch {
    console.error(`${ansi.red}No checkpoint found for transitionId "${transitionId}" in ${checkpointsRootFor(lingxiHome)}.${ansi.reset}`);
    console.error(`${ansi.dim}Run \`hana data checkpoints\` to see what is available.${ansi.reset}`);
    return 1;
  }
  const totalBytes = Array.isArray(metadata.items) ? metadata.items.reduce((sum, item) => sum + (item.bytes || 0), 0) : 0;
  console.log(`About to restore checkpoint ${ansi.bold}${transitionId}${ansi.reset}`);
  console.log(`  Epoch    ${metadata.fromEpoch ?? "unknown"} \u2192 ${metadata.toEpoch ?? "unknown"}`);
  console.log(`  Created  ${metadata.createdAt ?? "unknown"}`);
  console.log(`  Size     ${formatBytes(totalBytes)}`);
  console.log("");
  console.log(`${ansi.red}WARNING: this discards any changes made after this checkpoint's upgrade.${ansi.reset}`);
  console.log(`${ansi.red}Pre-restore data is moved into a quarantine directory, never deleted \u2014 it can still be recovered by hand afterward.${ansi.reset}`);
  console.log("");
  const expectedToken = `restore ${transitionId}`;
  let token = confirmToken;
  if (token === null) {
    if (!isTTY) {
      console.error(`${ansi.red}Refusing to restore without confirmation: stdin is not a TTY.${ansi.reset}`);
      console.error(`${ansi.dim}Pass --confirm-token "${expectedToken}" to confirm non-interactively. There is no flag that skips this.${ansi.reset}`);
      return 1;
    }
    token = await promptConfirmation(`Type "${expectedToken}" to confirm: `);
  }
  if (token !== expectedToken) {
    console.error(`${ansi.red}Confirmation did not match "${expectedToken}". Aborting; nothing was changed.${ansi.reset}`);
    return 1;
  }
  try {
    const result = await restore({
      homeDir: lingxiHome,
      transitionId,
      confirmToken: token,
      log: { warn: (msg) => console.error(`${ansi.dim}${msg}${ansi.reset}`) }
    });
    console.log(`${ansi.green}Restore complete.${ansi.reset}`);
    console.log(`  Receipt      ${result.receiptPath}`);
    console.log(`  Quarantine   ${result.quarantineDir}`);
    console.log("");
    console.log(`Reopen this data directory with the older kernel (epoch ${result.fromEpoch}) to continue.`);
    return 0;
  } catch (error) {
    console.error(`${ansi.red}Restore failed: ${error?.message ?? String(error)}${ansi.reset}`);
    return 1;
  }
}

// cli/entry.ts
var __dirname2 = path8.dirname(fileURLToPath3(import.meta.url));
var PROJECT_ROOT = path8.resolve(__dirname2, "..");
async function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseCliArgs(argv);
  } catch (err) {
    console.error(`${ansi.red}${err.message}${ansi.reset}`);
    console.log(helpText());
    return 1;
  }
  if (args.command === "help") {
    if (args.error) console.error(`${ansi.yellow}${args.error}${ansi.reset}
`);
    console.log(helpText());
    return args.error ? 1 : 0;
  }
  if (args.command === "serve") {
    if (args.runtime === "rust") {
      try {
        if (args.url || args.token) throw new Error("serve does not accept --url or --token");
        return await spawnRustServerForeground({
          projectRoot: PROJECT_ROOT,
          extraArgs: args.passthrough,
          channel: args.channel,
          allowDataDowngrade: args.allowDataDowngrade
        });
      } catch (err) {
        console.error(`${ansi.red}${safeRustTerminalText(err instanceof Error ? err.message : err)}${ansi.reset}`);
        return 1;
      }
    }
    await spawnServerForeground({
      projectRoot: PROJECT_ROOT,
      extraArgs: args.passthrough,
      channel: args.channel,
      allowDataDowngrade: args.allowDataDowngrade
    });
    return 0;
  }
  if (args.command === "bundle") {
    if (args.subcommand === "pull") {
      return await runBundlePull({ channel: args.channel });
    }
    return await runBundleStatus({ channel: args.channel });
  }
  if (args.command === "data") {
    if (args.subcommand === "diagnose") {
      return await runDataDiagnose();
    }
    if (args.subcommand === "checkpoints") {
      return await runDataCheckpoints();
    }
    return await runDataRestore({ transitionId: args.target, confirmToken: args.confirmToken });
  }
  if (args.runtime === "rust") {
    const connection2 = args.url ? explicitRustConnection(args.url, args.token || "") : readRustLocalService({ lingxiHome: resolveCliLingxiHome() });
    if (connection2.ok === false) {
      console.error(`${ansi.red}${safeRustTerminalText(connection2.message)}${ansi.reset}`);
      return 1;
    }
    const client2 = new RustCliClient(connection2);
    try {
      if (args.command === "status") {
        const health = await client2.health();
        let identity;
        let identityError;
        try {
          identity = await client2.identity();
        } catch (err) {
          identityError = err;
        }
        console.log("LingxiAgent Rust service");
        console.log(`  URL       ${safeRustTerminalText(connection2.baseUrl)}`);
        console.log(`  Version   ${safeRustTerminalText(health.serverVersion)}`);
        console.log(`  Studio    ${safeRustTerminalText(identity?.studioId || "unavailable")}`);
        console.log("  Agent     unavailable (Rust R02)");
        console.log("  Model     unavailable (Rust R02)");
        console.log(`  Auth      ${safeRustTerminalText(identity?.credentialKind || "unavailable (identity check failed)")}`);
        if (identityError || !identity?.studioId) {
          const detail = identityError instanceof Error ? identityError.message : "identity response lacks Studio";
          console.error(`${ansi.red}Rust status is incomplete: ${safeRustTerminalText(detail)}${ansi.reset}`);
          return 1;
        }
        console.error(`${ansi.red}Rust status is incomplete: Agent and model are not available yet${ansi.reset}`);
        return 1;
      }
      if (args.command === "sessions") {
        const sessions = await client2.sessions();
        if (sessions.length === 0) {
          console.log("No sessions yet.");
        } else {
          for (const [index, session] of sessions.slice(0, 20).entries()) {
            console.log(`${String(index + 1).padStart(2, " ")}. ${safeRustTerminalText(session.title, 72)} \xB7 ${safeRustTerminalText(session.agentId || "Agent", 72)}`);
          }
        }
        return 0;
      }
      if (args.command === "chat" || args.command === "continue") {
        await client2.health();
        await client2.identity();
        if (args.command === "continue" || args.session) {
          const sessions = await client2.sessions();
          const target = String(args.target || args.session || "").trim();
          const number = Number(target);
          const selected = !target ? sessions[0] : Number.isInteger(number) && number > 0 && String(number) === target ? sessions[number - 1] : sessions.find((session) => session.sessionId === target);
          if (!selected) throw new Error(`Session not found: ${target || "(empty)"}`);
          await client2.session(selected.sessionId);
        }
        throw new Error("Rust service cannot open CLI chat yet: session creation and model/tool reply streaming are unavailable");
      }
    } catch (err) {
      console.error(`${ansi.red}${safeRustTerminalText(err instanceof Error ? err.message : err)}${ansi.reset}`);
      return 1;
    }
  }
  let connection = resolveConnection({ url: args.url, token: args.token });
  if (!connection.ok && shouldAutoStartServer(args)) {
    console.error(`${ansi.dim}Starting local LingxiAgent Server...${ansi.reset}`);
    connection = await startLocalServerAndWait({ projectRoot: PROJECT_ROOT });
  }
  if (!connection.ok) {
    console.error(`${ansi.red}${connection.message}${ansi.reset}`);
    console.error(`${ansi.dim}Start one with: hana serve${ansi.reset}`);
    return 1;
  }
  const client = new LingxiCliClient(connection);
  try {
    if (args.command === "status") {
      await printStatus(client, connection);
      return 0;
    }
    if (args.command === "sessions") {
      await printSessions(client);
      return 0;
    }
    if (args.command === "continue") {
      await startChat(client, connection, { target: args.target, plain: args.plain });
      return 0;
    }
    if (args.command === "chat") {
      await startChat(client, connection, { session: args.session, plain: args.plain });
      return 0;
    }
  } catch (err) {
    console.error(`${ansi.red}${err instanceof Error ? err.message : String(err)}${ansi.reset}`);
    return 1;
  }
  console.log(helpText());
  return 0;
}
function shouldAutoStartServer(args) {
  if (args.url) return false;
  return args.command === "chat" || args.command === "continue";
}
if (process.argv[1] && path8.resolve(process.argv[1]) === fileURLToPath3(import.meta.url)) {
  const code = await main();
  if (code) process.exit(code);
}
export {
  main
};
