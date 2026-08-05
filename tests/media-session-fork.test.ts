import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { TaskStore } from "../core/media/task-store.ts";
import {
  collectRetainedMediaTaskState,
  rewriteForkedMediaTaskReferences,
} from "../core/media/session-fork.ts";

const tempDirs: string[] = [];
const sourceSessionPath = "/sessions/source.jsonl";
const childSessionPath = "/sessions/child.jsonl";

function makeStore() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-media-session-fork-"));
  tempDirs.push(dir);
  return new TaskStore(dir);
}

function addSourceTask(store: TaskStore, overrides: Record<string, any> = {}) {
  return store.add({
    taskId: "media-source",
    adapterId: "fake-image",
    providerId: "fake-provider",
    modelId: "fake-model",
    protocolId: "fake-images",
    credentialLaneId: "lane-1",
    batchId: "batch-source",
    type: "image",
    prompt: "draw a quiet notebook",
    params: { type: "image", prompt: "draw a quiet notebook" },
    sessionId: "session-source",
    sessionPath: sourceSessionPath,
    sessionRef: { sessionId: "session-source", sessionPath: sourceSessionPath },
    deliveryTarget: { kind: "bridge", platform: "test", chatId: "do-not-copy" },
    submitState: "submitted",
    adapterTaskId: "provider-job-source",
    ...overrides,
  });
}

function mediaPlaceholder(taskId = "media-source") {
  return {
    type: "message",
    message: {
      role: "toolResult",
      details: {
        mediaGeneration: {
          kind: "image",
          tasks: [{ taskId }],
        },
      },
    },
  };
}

function deferredRecord(taskId: string, status: string, extra: Record<string, any> = {}) {
  return {
    type: "custom",
    customType: "hana-deferred-result",
    data: {
      schemaVersion: 1,
      taskId,
      status,
      type: "image-generation",
      ...extra,
    },
  };
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("media Session Fork references", () => {
  it("collects and rewrites only typed media task references", () => {
    const entries: any[] = [
      mediaPlaceholder(),
      deferredRecord("media-source", "failed", { reason: "provider failed" }),
      { type: "file", fileId: "file-1", replacesTaskId: "media-source" },
      {
        type: "custom_message",
        customType: "hana-background-result",
        content: '<hana-background-result task-id="media-source" status="failed" type="image-generation">failed</hana-background-result>',
        details: { taskId: "media-source" },
      },
      { type: "interlude", variant: "deferred_result", taskId: "media-source" },
      { type: "subagent", taskId: "media-source", summary: "unrelated identity domain" },
      { type: "message", message: { role: "user", content: "keep media-source literally" } },
    ];

    expect(collectRetainedMediaTaskState(entries)).toMatchObject({
      taskIds: ["media-source"],
      resultsByTaskId: {
        "media-source": { status: "failed", reason: "failed" },
      },
    });

    const rewritten: any[] = rewriteForkedMediaTaskReferences(entries, {
      "media-source": "media-child",
    });
    expect(rewritten[0].message.details.mediaGeneration.tasks[0].taskId).toBe("media-child");
    expect(rewritten[1].data.taskId).toBe("media-child");
    expect(rewritten[2].replacesTaskId).toBe("media-child");
    expect(rewritten[3].content).toContain('task-id="media-child"');
    expect(rewritten[3].details.taskId).toBe("media-child");
    expect(rewritten[4].taskId).toBe("media-child");
    expect(rewritten[5].taskId).toBe("media-source");
    expect(rewritten[6].message.content).toBe("keep media-source literally");
  });
});

describe("TaskStore media Session Fork", () => {
  it("clones retained failed tasks under child ownership and keeps source/tail independent", () => {
    const store = makeStore();
    addSourceTask(store);
    store.update("media-source", {
      status: "failed",
      failReason: "provider failed",
      completedAt: "2026-07-18T12:00:00.000Z",
    });
    addSourceTask(store, { taskId: "media-tail", batchId: "batch-tail" });

    const forked = store.forkSessionTasks({
      sourceSessionId: "session-source",
      sourceSessionPath,
      targetSessionId: "session-child",
      targetSessionPath: childSessionPath,
      retainedEntries: [
        mediaPlaceholder(),
        deferredRecord("media-source", "failed", { reason: "provider failed" }),
      ],
      createTaskId: () => "media-child",
      createBatchId: () => "batch-child",
    });

    expect(forked).toMatchObject({
      tasks: 1,
      taskIds: ["media-child"],
      taskIdMap: { "media-source": "media-child" },
      deferredRecords: [{
        taskId: "media-child",
        status: "failed",
        type: "image-generation",
        reason: "provider failed",
      }],
    });
    expect(store.get("media-child")).toMatchObject({
      sessionId: "session-child",
      sessionPath: childSessionPath,
      sessionRef: { sessionId: "session-child", sessionPath: childSessionPath },
      status: "failed",
      adapterTaskId: null,
      deliveryMode: "session",
      delivery: { mode: "session" },
      deliveryTarget: null,
      forkedFromTaskId: "media-source",
    });
    expect(store.get("media-source")).toMatchObject({
      status: "failed",
      adapterTaskId: "provider-job-source",
      sessionId: "session-source",
    });
    expect(forked.taskIdMap).not.toHaveProperty("media-tail");

    expect(store.discardForkedSessionTasks({
      targetSessionId: "session-child",
      taskIds: forked.taskIds,
    })).toEqual({ discarded: 1, skipped: [] });
    expect(store.get("media-child")).toBeNull();
    expect(store.get("media-source")).toBeTruthy();
  });

  it("turns a task that was pending at the retained boundary into a child-owned retryable abort", () => {
    const store = makeStore();
    addSourceTask(store, {
      params: {
        type: "image",
        prompt: "draw a quiet notebook",
        image: "/cache/source/reference.png",
      },
    });

    const forked = store.forkSessionTasks({
      sourceSessionId: "session-source",
      sourceSessionPath,
      targetSessionId: "session-child",
      targetSessionPath: childSessionPath,
      retainedEntries: [mediaPlaceholder()],
      forkedSessionFiles: [{
        id: "file-child",
        fileId: "file-child",
        sessionId: "session-child",
        sessionPath: childSessionPath,
        filePath: "/cache/child/reference.png",
        realPath: "/cache/child/reference.png",
        legacyFileIds: ["file-source"],
        legacyFilePaths: ["/cache/source/reference.png"],
      }],
      createTaskId: () => "media-child",
      createBatchId: () => "batch-child",
    });

    expect(store.get("media-child")).toMatchObject({
      status: "aborted",
      submitState: "failed",
      adapterTaskId: null,
      files: [],
      sessionFiles: [],
      params: { image: "/cache/child/reference.png" },
      failReason: expect.stringContaining("still running at the Fork point"),
    });
    expect(forked.deferredRecords).toEqual([
      expect.objectContaining({
        taskId: "media-child",
        status: "aborted",
        reason: expect.stringContaining("Retry"),
      }),
    ]);
    expect(store.get("media-source")).toMatchObject({
      status: "pending",
      adapterTaskId: "provider-job-source",
      params: { image: "/cache/source/reference.png" },
    });
  });

  it("keeps a retained completed result readable through child-owned SessionFiles without sharing generated-file deletion", () => {
    const store = makeStore();
    addSourceTask(store);
    const sourceFile = {
      id: "file-source",
      fileId: "file-source",
      sessionId: "session-source",
      sessionPath: sourceSessionPath,
      filePath: "/generated/source.png",
      realPath: "/generated/source.png",
    };
    store.update("media-source", {
      status: "done",
      files: ["source.png"],
      sessionFiles: [sourceFile],
      imageWidth: 1024,
      imageHeight: 768,
      completedAt: "2026-07-18T12:00:00.000Z",
    });
    const childFile = {
      id: "file-child",
      fileId: "file-child",
      sessionId: "session-child",
      sessionPath: childSessionPath,
      filePath: "/generated/child.png",
      realPath: "/generated/child.png",
      legacyFileIds: ["file-source"],
      legacyFilePaths: ["/generated/source.png"],
    };

    const forked = store.forkSessionTasks({
      sourceSessionId: "session-source",
      sourceSessionPath,
      targetSessionId: "session-child",
      targetSessionPath: childSessionPath,
      retainedEntries: [
        mediaPlaceholder(),
        deferredRecord("media-source", "success", {
          result: { sessionFiles: [sourceFile] },
        }),
      ],
      forkedSessionFiles: [childFile],
      createTaskId: () => "media-child",
      createBatchId: () => "batch-child",
    });

    expect(store.get("media-child")).toMatchObject({
      status: "done",
      files: [],
      sessionFiles: [expect.objectContaining({ fileId: "file-child", sessionId: "session-child" })],
      imageWidth: 1024,
      imageHeight: 768,
    });
    expect(forked.deferredRecords).toEqual([
      expect.objectContaining({
        taskId: "media-child",
        status: "success",
        result: {
          sessionFiles: [expect.objectContaining({ fileId: "file-child" })],
        },
      }),
    ]);
    expect(store.get("media-source")).toMatchObject({
      status: "done",
      files: ["source.png"],
      sessionFiles: [expect.objectContaining({ fileId: "file-source" })],
    });
  });

  it("rolls back earlier child tasks if a later clone cannot allocate an independent id", () => {
    const store = makeStore();
    addSourceTask(store, { taskId: "media-one", batchId: "batch-one" });
    addSourceTask(store, { taskId: "media-two", batchId: "batch-two" });

    expect(() => store.forkSessionTasks({
      sourceSessionId: "session-source",
      sourceSessionPath,
      targetSessionId: "session-child",
      targetSessionPath: childSessionPath,
      retainedEntries: [mediaPlaceholder("media-one"), mediaPlaceholder("media-two")],
      createTaskId: () => "media-child-collision",
      createBatchId: (_task: unknown, index: number) => `batch-child-${index}`,
    })).toThrow("not unique");

    expect(store.get("media-child-collision")).toBeNull();
    expect(store.listAll().map((task) => task.taskId).sort()).toEqual(["media-one", "media-two"]);
  });

  it("fails explicitly and rolls back memory when the child task snapshot cannot persist", () => {
    const store = makeStore();
    addSourceTask(store);
    (store as any)._writeSync = () => false;

    expect(() => store.forkSessionTasks({
      sourceSessionId: "session-source",
      sourceSessionPath,
      targetSessionId: "session-child",
      targetSessionPath: childSessionPath,
      retainedEntries: [mediaPlaceholder()],
      createTaskId: () => "media-child",
      createBatchId: () => "batch-child",
    })).toThrow("could not be persisted");

    expect(store.get("media-child")).toBeNull();
    expect(store.get("media-source")).toBeTruthy();
    store.destroy();
  });
});
