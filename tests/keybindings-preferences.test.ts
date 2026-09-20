import { describe, expect, it } from "vitest";
import {
  KEYBINDING_COMMANDS,
  defaultKeybindings,
  findKeybindingConflicts,
  getEffectiveKeybindings,
  mergeKeybindings,
  normalizeBinding,
  normalizeKeybindings,
} from "../shared/keybindings-preferences.cjs";

describe("keybindings-preferences", () => {
  it("registers every command with defaults and a scope", () => {
    expect(KEYBINDING_COMMANDS.length).toBeGreaterThanOrEqual(6);
    const ids = KEYBINDING_COMMANDS.map(c => c.id);
    expect(new Set(ids).size).toBe(ids.length); // id 唯一
    for (const command of KEYBINDING_COMMANDS) {
      expect(command.defaultKeys.length).toBeGreaterThan(0);
      expect(["global", "app", "local"]).toContain(command.scope);
    }
  });

  it("normalizes bindings: Space aliasing, modifier aliases, dedupe, and default-drop", () => {
    expect(normalizeBinding("CmdOrCtrl + shift + m")).toBe("CommandOrControl+Shift+M");
    expect(normalizeBinding("Alt + \u00A0")).toBe("Alt+Space");
    expect(normalizeBinding("  ")).toBe("");

    // 等于默认值的覆盖项不落盘；未知命令剔除；数组去重
    const defaults = defaultKeybindings();
    expect(normalizeKeybindings({
      "app.restart": defaults["app.restart"],
      "app.new-session": ["CommandOrControl+N", "CommandOrControl+N"],
      "bogus.command": ["F9"],
      "quick-chat.toggle": ["F9"],
    })).toEqual({ "quick-chat.toggle": ["F9"] });

    // 空数组是显式「未绑定」，保留
    expect(normalizeKeybindings({ "app.new-session": [] })).toEqual({ "app.new-session": [] });
  });

  it("merges stored overrides and falls back to defaults for untouched commands", () => {
    const stored = mergeKeybindings({ "quick-chat.toggle": ["F9"] }, { "app.open-settings": ["CommandOrControl+Shift+P"] });
    const effective = getEffectiveKeybindings(stored);
    expect(effective["quick-chat.toggle"]).toEqual(["F9"]);
    expect(effective["app.open-settings"]).toEqual(["CommandOrControl+Shift+P"]);
    // 未覆盖命令回落默认
    expect(effective["app.new-session"]).toEqual(defaultKeybindings()["app.new-session"]);
  });

  it("detects conflicts against other commands only", () => {
    // 与快捷对话默认键位冲突
    expect(findKeybindingConflicts("app.new-session", ["Alt+Space"], {})).toEqual(["quick-chat.toggle"]);
    // 同命令自身的键位不算冲突
    expect(findKeybindingConflicts("quick-chat.toggle", ["Alt+Space"], {})).toEqual([]);
    // 覆盖后的键位参与冲突判定
    expect(findKeybindingConflicts("app.restart", ["F9"], { "quick-chat.toggle": ["F9"] })).toEqual(["quick-chat.toggle"]);
  });
});
