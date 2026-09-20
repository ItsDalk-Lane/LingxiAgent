import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Agent } from "../core/agent.ts";

const tempDirs = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-agent-platform-prompt-"));
  tempDirs.push(dir);
  return dir;
}

function makeAgent(locale) {
  const root = makeTempDir();
  const agentsDir = path.join(root, "agents");
  const productDir = path.join(root, "product");
  const userDir = path.join(root, "user");
  fs.mkdirSync(path.join(agentsDir, "hana"), { recursive: true });
  fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  fs.writeFileSync(path.join(productDir, "yuan", "lingxi.md"), "Yuan prompt", "utf-8");

  const agent = new Agent({ id: "hana", agentsDir, productDir, userDir } as any);
  agent._config = {
    locale,
    agent: { yuan: "lingxi" },
    memory: { enabled: false },
    experience: { enabled: false },
  };
  agent.userName = locale.startsWith("zh") ? "用户" : "User";
  agent.agentName = "Hanako";
  return agent;
}

function writeUserProfile(agent, content) {
  fs.writeFileSync(path.join(agent.userDir, "user.md"), content, "utf-8");
}

afterEach(() => {
  vi.useRealTimers();
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("Agent platform prompt identity", () => {
  it("describes the Lingxi platform identity in Chinese without upstream references", () => {
    const prompt = makeAgent("zh-CN").buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("你运行在灵犀（Lingxi）平台上");
    expect(prompt).not.toContain("ItsDalk-Lane");
    expect(prompt).not.toContain("LingxiAgent");
  });

  it("describes the Lingxi platform identity in English without upstream references", () => {
    const prompt = makeAgent("en").buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("You are running on the Lingxi (灵犀) platform");
    expect(prompt).not.toContain("ItsDalk-Lane");
    expect(prompt).not.toContain("LingxiAgent");
  });

  it("distinguishes SessionFile identity from writable local refs in Chinese", () => {
    const prompt = makeAgent("zh-CN").buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("会话文件优先用 fileId 操作，label 仅展示");
    expect(prompt).toContain("write/edit 用 writableLocalRef.path 或本机路径，不接受 fileId；");
    expect(prompt).toContain("materialize");
  });

  it("distinguishes SessionFile identity from writable local refs in English", () => {
    const prompt = makeAgent("en").buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("Operate on session files by fileId; label is display-only.");
    expect(prompt).toContain("write/edit takes writableLocalRef.path or local paths, never fileId;");
    expect(prompt).toContain("materialize");
  });

  it("formats prompt times with an unambiguous 24-hour clock", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-04T07:53:00.000Z"));

    const agent = makeAgent("en");
    agent._cb = { getTimezone: () => "Asia/Shanghai" };

    const prompt = agent.buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("Session started at:");
    expect(prompt).toContain("current_status");
    expect(prompt).not.toContain("Current date and time:");
    expect(prompt).toContain("15:53");
    expect(prompt).toContain("Memory/diary archives use the 04:00 boundary");
    expect(prompt).toContain("ordinary dates and schedules follow the user's timezone calendar.");
    expect(prompt).not.toMatch(/\b(?:AM|PM)\b/);
  });

  it("injects the configured Chinese user name as an explicit profile fact", () => {
    const agent = makeAgent("zh-CN");
    agent._cb = { getTimezone: () => "Asia/Shanghai", getUserName: () => "黎" };
    agent.userName = "黎";
    writeUserProfile(agent, "喜欢安静、克制的界面。\n");

    const prompt = agent.buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("# 用户档案");
    expect(prompt).toContain("用户的名字叫：黎");
    expect(prompt).toContain("喜欢安静、克制的界面。");
    expect(prompt).not.toContain("由用户手动维护");
  });

  it("injects the configured English user name as an explicit profile fact", () => {
    const agent = makeAgent("en");
    agent._cb = { getTimezone: () => "Asia/Shanghai", getUserName: () => "Li" };
    agent.userName = "Li";
    writeUserProfile(agent, "Prefers quiet interfaces.\n");

    const prompt = agent.buildSystemPrompt({
      forceMemoryEnabled: false,
      forceExperienceEnabled: false,
    });

    expect(prompt).toContain("# User Profile");
    expect(prompt).toContain("The user's name is: Li");
    expect(prompt).toContain("Prefers quiet interfaces.");
    expect(prompt).not.toContain("manually maintained");
  });
});
