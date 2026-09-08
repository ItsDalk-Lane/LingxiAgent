import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { TaskStore } from "../core/media/task-store.ts";

const roots: string[] = [];

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-r09-"));
  roots.push(root);
  const generatedDir = path.join(root, "generated");
  fs.mkdirSync(generatedDir, { recursive: true });
  const store = new TaskStore(path.join(root, "store"));
  return { root, generatedDir, store };
}

function addPending(store: TaskStore, taskId: string, files: string[] = []) {
  store.add({
    taskId,
    adapterId: "openai-speech",
    providerId: "openai",
    modelId: "tts-1",
    protocolId: "openai-audio-speech",
    batchId: "batch-r09",
    type: "speech",
    prompt: "合成测试",
    params: { type: "speech", prompt: "合成测试" },
    deliveryMode: "response",
    delivery: { mode: "response" },
  });
  if (files.length > 0) store.update(taskId, { files });
}

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("R09 同步语音产物真实文件边界", () => {
  it("R09-01: files=['.'] 与实际子目录都拒绝，不能标 done", () => {
    const { generatedDir, store } = fixture();
    fs.mkdirSync(path.join(generatedDir, "nested"));
    for (const [taskId, file] of [["dot", "."], ["directory", "nested"]]) {
      addPending(store, taskId);
      const result = store.completeSynchronousSpeechTask(taskId, { files: [file], generatedDir });
      expect(result.ok).toBe(false);
      expect(store.get(taskId)?.status).toBe("failed");
    }
    store.destroy();
  });

  it("R09-02/03: 外链拒绝，指向根内普通文件的链接按既定规则接受", () => {
    const { root, generatedDir, store } = fixture();
    const outside = path.join(root, "outside.mp3");
    fs.writeFileSync(outside, "outside");
    fs.symlinkSync(outside, path.join(generatedDir, "outside-link.mp3"));
    fs.writeFileSync(path.join(generatedDir, "inside.mp3"), "inside");
    fs.symlinkSync("inside.mp3", path.join(generatedDir, "inside-link.mp3"));

    addPending(store, "outside-link");
    const rejected = store.completeSynchronousSpeechTask("outside-link", {
      files: ["outside-link.mp3"], generatedDir,
    });
    expect(rejected.ok).toBe(false);
    expect(store.get("outside-link")?.status).toBe("failed");
    expect((rejected as any).error).not.toContain(outside);

    addPending(store, "inside-link");
    const accepted = store.completeSynchronousSpeechTask("inside-link", {
      files: ["inside-link.mp3"], generatedDir,
    });
    expect(accepted.ok).toBe(true);
    expect(store.get("inside-link")?.files).toEqual(["inside-link.mp3"]);
    store.destroy();
  });

  it("R09-03: generated 根本身是合法目录链接时先 canonicalize，再接受根内普通文件", () => {
    const { root, store } = fixture();
    const realGenerated = path.join(root, "real-generated");
    const linkedGenerated = path.join(root, "linked-generated");
    fs.mkdirSync(realGenerated);
    fs.writeFileSync(path.join(realGenerated, "inside.m4a"), "inside");
    fs.symlinkSync(realGenerated, linkedGenerated);
    addPending(store, "linked-root");
    const result = store.completeSynchronousSpeechTask("linked-root", {
      files: ["inside.m4a"], generatedDir: linkedGenerated,
    });
    expect(result.ok).toBe(true);
    expect(store.get("linked-root")?.files).toEqual(["inside.m4a"]);
    store.destroy();
  });

  it("R09-04: 悬空与循环链接明确失败，不抛出未处理文件系统异常", () => {
    const { generatedDir, store } = fixture();
    fs.symlinkSync("missing.mp3", path.join(generatedDir, "dangling.mp3"));
    fs.symlinkSync("loop-b.mp3", path.join(generatedDir, "loop-a.mp3"));
    fs.symlinkSync("loop-a.mp3", path.join(generatedDir, "loop-b.mp3"));
    for (const [taskId, file] of [["dangling", "dangling.mp3"], ["loop", "loop-a.mp3"]]) {
      addPending(store, taskId);
      const result = store.completeSynchronousSpeechTask(taskId, { files: [file], generatedDir });
      expect(result.ok).toBe(false);
      expect((result as any).error).toMatch(/speech output/);
      expect(store.get(taskId)?.status).toBe("failed");
    }
    store.destroy();
  });

  it("R09-05: root/root-evil 前缀、../ 与绝对路径都拒绝", () => {
    const { root, generatedDir, store } = fixture();
    const evilDir = `${generatedDir}-evil`;
    fs.mkdirSync(evilDir);
    const evilFile = path.join(evilDir, "evil.mp3");
    fs.writeFileSync(evilFile, "evil");
    fs.writeFileSync(path.join(generatedDir, "inside.mp3"), "inside");
    const cases = [
      ["prefix", path.join("..", path.basename(evilDir), "evil.mp3")],
      ["dotdot", "../outside.mp3"],
      ["absolute", path.join(generatedDir, "inside.mp3")],
    ];
    for (const [taskId, file] of cases) {
      addPending(store, taskId);
      const result = store.completeSynchronousSpeechTask(taskId, { files: [file], generatedDir });
      expect(result.ok).toBe(false);
      expect(store.get(taskId)?.status).toBe("failed");
    }
    store.destroy();
  });

  it("R09-06/07/08: 普通文件成功；多文件一个无效则整体失败；终态不倒退", () => {
    const { generatedDir, store } = fixture();
    fs.writeFileSync(path.join(generatedDir, "a.mp3"), "a");
    fs.writeFileSync(path.join(generatedDir, "b.wav"), "b");
    fs.mkdirSync(path.join(generatedDir, "not-file"));

    addPending(store, "valid");
    const valid = store.completeSynchronousSpeechTask("valid", {
      files: ["a.mp3", "b.wav"], generatedDir,
    });
    expect(valid.ok).toBe(true);
    const completedAt = store.get("valid")?.completedAt;
    expect(store.completeSynchronousSpeechTask("valid", { files: ["a.mp3"], generatedDir }).ok).toBe(true);
    expect(store.get("valid")?.completedAt).toBe(completedAt);

    addPending(store, "mixed");
    const mixed = store.completeSynchronousSpeechTask("mixed", {
      files: ["a.mp3", "not-file"], generatedDir,
    });
    expect(mixed.ok).toBe(false);
    expect(store.get("mixed")?.status).toBe("failed");
    expect(store.get("mixed")?.files).toEqual([]);
    store.destroy();
  });

  it("R09-09/10: 重启恢复与新完成共用同一规则，目录和外链失败、普通文件完成", () => {
    const { root, generatedDir, store } = fixture();
    fs.writeFileSync(path.join(generatedDir, "ok.mp3"), "ok");
    fs.mkdirSync(path.join(generatedDir, "fake.mp3"));
    const outside = path.join(root, "outside.mp3");
    fs.writeFileSync(outside, "outside");
    fs.symlinkSync(outside, path.join(generatedDir, "escape.mp3"));
    addPending(store, "recover-ok", ["ok.mp3"]);
    addPending(store, "recover-dir", ["fake.mp3"]);
    addPending(store, "recover-link", ["escape.mp3"]);

    expect(store.recoverSynchronousSpeechTasks({ generatedDir })).toBe(3);
    expect(store.get("recover-ok")?.status).toBe("done");
    expect(store.get("recover-dir")?.status).toBe("failed");
    expect(store.get("recover-link")?.status).toBe("failed");
    store.flushSync();
    store.destroy();

    const restarted = new TaskStore(path.join(root, "store"));
    expect(restarted.get("recover-ok")?.status).toBe("done");
    expect(restarted.get("recover-dir")?.status).toBe("failed");
    expect(restarted.get("recover-link")?.status).toBe("failed");
    restarted.destroy();
  });
});
