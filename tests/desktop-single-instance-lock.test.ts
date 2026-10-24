import { describe, expect, it, vi } from "vitest";
import path from "path";
import fs from "fs";
import os from "os";

import {
  configureClientSingleInstance,
  focusExistingWindow,
  getClientUserDataPath,
} from "../desktop/src/shared/single-instance-lock.cjs";

function makeApp({ gotLock = true } = {}) {
  const handlers = new Map();
  return {
    app: {
      getPath: vi.fn((name) => {
        if (name === "appData") return path.join("C:", "Users", "me", "AppData", "Roaming");
        return path.join("C:", "tmp");
      }),
      setPath: vi.fn(),
      requestSingleInstanceLock: vi.fn(() => gotLock),
      on: vi.fn((event, handler) => handlers.set(event, handler)),
      exit: vi.fn(),
      quit: vi.fn(),
    },
    handlers,
  };
}

describe("desktop client single instance lock", () => {
  it("不同位置的同名副本不共用缓存或单实例空间，路径别名仍指向同一空间", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "r08-client-isolation-"));
    try {
      const a = path.join(root, "a", "copy");
      const b = path.join(root, "b", "copy");
      fs.mkdirSync(a, { recursive: true });
      fs.mkdirSync(b, { recursive: true });
      const alias = path.join(root, "alias");
      fs.symlinkSync(a, alias, "junction");
      const defaults = path.join(root, ".hanako");
      const namespace = (home: string) => getClientUserDataPath(home, defaults, path.join(root, "appData"));
      expect(namespace(a)).not.toBe(namespace(b));
      expect(namespace(a)).toBe(namespace(alias));
      expect(namespace(a)).toBe(path.join(fs.realpathSync(a), ".electron-user-data"));
      const standard = path.join(root, ".lingxi-dev");
      fs.mkdirSync(standard);
      const standardAlias = path.join(root, "standard-alias");
      fs.symlinkSync(standard, standardAlias, "junction");
      expect(namespace(standardAlias)).toBe(namespace(standard));
      expect(namespace(standard)).toBe(path.join(root, "appData", "Lingxi-dev"));
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
  it("sets the dev userData namespace before requesting the lock", () => {
    const { app } = makeApp();
    const defaultHome = path.join("C:", "Users", "me", ".hanako");
    const devHome = path.join("C:", "Users", "me", ".hanako-dev");

    const acquired = configureClientSingleInstance(app, {
      lingxiHome: devHome,
      defaultHome,
      onSecondInstance: vi.fn(),
    });

    expect(acquired).toBe(true);
    expect(app.setPath).toHaveBeenCalledWith(
      "userData",
      path.join("C:", "Users", "me", "AppData", "Roaming", "Hanako-dev"),
    );
    expect(app.setPath.mock.invocationCallOrder[0]).toBeLessThan(
      app.requestSingleInstanceLock.mock.invocationCallOrder[0],
    );
  });

  it("keeps production in the legacy Electron userData namespace", () => {
    const { app } = makeApp();
    const defaultHome = path.join("C:", "Users", "me", ".hanako");

    const acquired = configureClientSingleInstance(app, {
      lingxiHome: defaultHome,
      defaultHome,
      onSecondInstance: vi.fn(),
    });

    expect(acquired).toBe(true);
    expect(app.setPath).toHaveBeenCalledWith(
      "userData",
      path.join("C:", "Users", "me", "AppData", "Roaming", "Hanako"),
    );
    expect(app.requestSingleInstanceLock).toHaveBeenCalledTimes(1);
  });

  it("exits a duplicate client before registering second-instance handlers", () => {
    const { app } = makeApp({ gotLock: false });

    const acquired = configureClientSingleInstance(app, {
      lingxiHome: path.join("C:", "Users", "me", ".hanako"),
      defaultHome: path.join("C:", "Users", "me", ".hanako"),
      onSecondInstance: vi.fn(),
    });

    expect(acquired).toBe(false);
    expect(app.exit).toHaveBeenCalledWith(0);
    expect(app.quit).not.toHaveBeenCalled();
    expect(app.on).not.toHaveBeenCalledWith("second-instance", expect.any(Function));
  });

  it("brings the primary window forward when a duplicate launch is redirected", () => {
    const { app, handlers } = makeApp();
    const onSecondInstance = vi.fn();

    configureClientSingleInstance(app, {
      lingxiHome: path.join("C:", "Users", "me", ".hanako"),
      defaultHome: path.join("C:", "Users", "me", ".hanako"),
      onSecondInstance,
    });

    handlers.get("second-instance")?.();

    expect(onSecondInstance).toHaveBeenCalledTimes(1);
  });

  it("restores, shows, and focuses a hidden or minimized window", () => {
    const win = {
      isDestroyed: vi.fn(() => false),
      isMinimized: vi.fn(() => true),
      restore: vi.fn(),
      show: vi.fn(),
      focus: vi.fn(),
    };

    expect(focusExistingWindow(win)).toBe(true);
    expect(win.restore).toHaveBeenCalledTimes(1);
    expect(win.show).toHaveBeenCalledTimes(1);
    expect(win.focus).toHaveBeenCalledTimes(1);
  });
});
