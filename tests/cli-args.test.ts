import { describe, expect, it } from "vitest";
import { parseCliArgs } from "../cli/args.ts";

describe("CLI args", () => {
  it("defaults to help when no command is provided", () => {
    expect(parseCliArgs([])).toMatchObject({ command: "help" });
  });

  it("parses chat connection options", () => {
    expect(parseCliArgs(["chat", "--plain", "--url", "http://host:14500", "--token", "abc", "--session", "s1"])).toMatchObject({
      command: "chat",
      plain: true,
      url: "http://host:14500",
      token: "abc",
      session: "s1",
    });
  });

  it("parses continue target as index or session path", () => {
    expect(parseCliArgs(["continue", "2"])).toMatchObject({
      command: "continue",
      target: "2",
    });
    expect(parseCliArgs(["continue", "/tmp/session.json"])).toMatchObject({
      command: "continue",
      target: "/tmp/session.json",
    });
  });

  it("keeps server args after -- as passthrough", () => {
    expect(parseCliArgs(["serve", "--", "--chat"])).toMatchObject({
      command: "serve",
      passthrough: ["--chat"],
    });
  });

  it("rejects missing option values", () => {
    expect(() => parseCliArgs(["chat", "--url"])).toThrow("--url requires a value");
  });

  it("defaults allowDataDowngrade to false and flips it on --allow-data-downgrade", () => {
    expect(parseCliArgs(["serve"])).toMatchObject({ command: "serve", allowDataDowngrade: false });
    expect(parseCliArgs(["serve", "--allow-data-downgrade"])).toMatchObject({
      command: "serve",
      allowDataDowngrade: true,
    });
  });

  it("selects Rust only when explicitly requested", () => {
    expect(parseCliArgs(["status"])).toMatchObject({ runtime: "node" });
    expect(parseCliArgs(["status", "--runtime", "rust"])).toMatchObject({ runtime: "rust" });
    expect(() => parseCliArgs(["status", "--runtime", "invalid"])).toThrow("--runtime must be one of");
  });

  it("rejects options that would be ignored by their command", () => {
    expect(parseCliArgs(["serve", "--url", "http://localhost"])).toMatchObject({
      command: "help", error: "--url is not supported for hana serve",
    });
    expect(parseCliArgs(["sessions", "--channel", "beta"])).toMatchObject({
      command: "help", error: "--channel is not supported for hana sessions",
    });
    expect(parseCliArgs(["status", "--token", "secret"])).toMatchObject({
      command: "help", error: "--token requires --url",
    });
    expect(parseCliArgs(["bundle", "status", "--runtime", "rust"])).toMatchObject({
      command: "help", error: "--runtime is not supported for hana bundle",
    });
    expect(() => parseCliArgs(["status", "--runtime", "rust", "--runtime", "node"]))
      .toThrow("--runtime was given more than once");
  });
});
