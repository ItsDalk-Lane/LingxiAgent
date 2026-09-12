import path from "path";
import { constants } from "fs";
import { AsyncLocalStorage } from "async_hooks";
import type { ResourceIO } from "./resource-io.ts";
import { normalizeResourceRef } from "./resource-refs.ts";
import type { ResourceRef } from "./types.ts";
import type { ToolFileChangePresentation } from "../../shared/tool-presentation.ts";
import {
  FILE_PRESENTATION_MAX_BYTES,
  fileChangePresentation,
  previousFileContent,
  retainAppliedPatch,
  type BeforeFileContent,
} from "./file-change-presentation.ts";

type ToolOperationsOptions = {
  cwd: string;
  resourceIO: ResourceIO;
  getSessionPath?: () => string | null;
  getSessionIdentity?: () => {
    sessionId?: string | null;
    sessionPath?: string | null;
    userId?: string | null;
    studioId?: string | null;
  };
  detectImageMimeType?: (filePath: string) => Promise<string | undefined> | string | undefined;
};

type TargetBinding = {
  rootPath: string;
  ref: ResourceRef;
};

type BoundTargetOptions = {
  rootPath: string;
  ref: unknown;
};

const targetBindingStorage = new AsyncLocalStorage<TargetBinding[]>();

function localRef(filePath: string): ResourceRef {
  return { kind: "local-file" as const, path: filePath };
}

function filePathFromRefPath(filePath: string, cwd: string): string {
  return path.isAbsolute(filePath) ? path.normalize(filePath) : path.resolve(cwd, filePath);
}

function isInside(rootPath: string, candidatePath: string): boolean {
  const relative = path.relative(rootPath, candidatePath);
  return relative === "" || (!!relative && !relative.startsWith("..") && !path.isAbsolute(relative));
}

function toSlashPath(filePath: string): string {
  return filePath.split(path.sep).join("/");
}

function joinResourcePath(rootPath: string, relativePath: string): string {
  const root = String(rootPath || "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  const child = String(relativePath || "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  return [root, child].filter(Boolean).join("/");
}

function refForBoundPath(binding: TargetBinding, filePath: string): ResourceRef | null {
  const absolute = path.normalize(filePath);
  if (!isInside(binding.rootPath, absolute)) return null;
  const relative = path.relative(binding.rootPath, absolute);
  if (binding.ref.kind === "mount") {
    return {
      kind: "mount",
      mountId: binding.ref.mountId,
      path: joinResourcePath(binding.ref.path, toSlashPath(relative)),
    };
  }
  if (binding.ref.kind === "local-file") {
    return {
      kind: "local-file",
      path: relative ? path.join(binding.ref.path, relative) : binding.ref.path,
    };
  }
  if (relative) return null;
  return binding.ref;
}

function statLike(result: Awaited<ReturnType<ResourceIO["stat"]>>) {
  return {
    isDirectory: () => result.isDirectory,
    isFile: () => result.exists && !result.isDirectory,
    size: result.version?.size ?? 0,
    mtimeMs: result.version?.mtimeMs ?? 0,
  };
}

export function createResourceIoToolOperations({
  cwd,
  resourceIO,
  getSessionPath = () => null,
  getSessionIdentity,
  detectImageMimeType,
}: ToolOperationsOptions) {
  const mutationCapture = new AsyncLocalStorage<{
    kind: "write" | "edit";
    before?: BeforeFileContent;
    fileChange?: ToolFileChangePresentation;
  }>();
  const operationContext = (reason: string, extra: Record<string, unknown> = {}) => {
    const identity = typeof getSessionIdentity === "function"
      ? getSessionIdentity() || {}
      : { sessionPath: getSessionPath() };
    const sessionPath = identity.sessionPath ?? getSessionPath() ?? null;
    const sessionId = identity.sessionId ?? null;
    return {
      ...extra,
      source: "agent_tool" as const,
      reason,
      sessionId,
      sessionPath,
      principal: {
        kind: "agent" as const,
        sessionId,
        sessionPath,
        userId: identity.userId ?? null,
        studioId: identity.studioId ?? null,
      },
    };
  };

  const refForPath = (filePath: string): ResourceRef => {
    const absolute = filePathFromRefPath(filePath, cwd);
    const bindings = targetBindingStorage.getStore() || [];
    const sortedBindings = [...bindings].sort((a, b) => b.rootPath.length - a.rootPath.length);
    for (const binding of sortedBindings) {
      const ref = refForBoundPath(binding, absolute);
      if (ref) return ref;
    }
    return localRef(absolute);
  };

  const hasBoundTarget = (filePath: string): boolean => {
    const absolute = filePathFromRefPath(filePath, cwd);
    return (targetBindingStorage.getStore() || []).some((binding) => Boolean(refForBoundPath(binding, absolute)));
  };

  const withResourceTarget = async <T>({ rootPath, ref }: BoundTargetOptions, fn: () => Promise<T>): Promise<T> => {
    const binding = {
      rootPath: filePathFromRefPath(rootPath, cwd),
      ref: normalizeResourceRef(ref),
    };
    const current = targetBindingStorage.getStore() || [];
    return targetBindingStorage.run([...current, binding], fn);
  };

  const readFile = async (filePath: string) => {
    const result = await resourceIO.read(refForPath(filePath));
    const capture = mutationCapture.getStore();
    if (capture?.kind === "edit") capture.before = previousFileContent(result.content);
    return result.content;
  };

  const access = async (filePath: string, _mode = constants.R_OK) => {
    const stat = await resourceIO.stat(refForPath(filePath));
    if (!stat.exists) throw new Error(`Path not found: ${filePath}`);
  };

  const writeFile = async (filePath: string, content: string | Buffer) => {
    const capture = mutationCapture.getStore();
    const ref = refForPath(filePath);
    let before: BeforeFileContent = {};
    if (capture) {
      // SDK 在同一文件的写入队列内调用此操作；旧内容读取必须留在这里。
      try {
        const stat = await resourceIO.stat(ref);
        before = (stat.version?.size ?? 0) > FILE_PRESENTATION_MAX_BYTES
          ? { reason: "before_content_too_large" }
          : previousFileContent((await resourceIO.read(ref)).content);
      } catch (error) {
        const code = typeof error === "object" && error !== null && "code" in error ? String(error.code) : "";
        before = { reason: /DENIED|FORBIDDEN|EACCES|EPERM/i.test(code) ? "before_permission_denied" : "before_content_unavailable" };
      }
    }
    const result = await resourceIO.write(ref, content, operationContext("agent_write"));
    if (capture) capture.fileChange = fileChangePresentation({
      filePath,
      content,
      before,
      changeType: result.changeType,
      generatePatch: true,
    });
  };

  const editWriteFile = async (filePath: string, content: string | Buffer) => {
    const result = await resourceIO.write(refForPath(filePath), content, operationContext("agent_edit"));
    const capture = mutationCapture.getStore();
    if (capture) capture.fileChange = fileChangePresentation({
      filePath,
      content,
      before: capture.before ?? {},
      changeType: result.changeType,
      // 编辑的真实补丁由 SDK 返回，不再次计算或用模型参数猜测。
      generatePatch: false,
    });
  };

  const withFileChangeCapture = async (kind: "write" | "edit", execute: () => Promise<any>) => {
    const capture: { kind: "write" | "edit"; before?: BeforeFileContent; fileChange?: ToolFileChangePresentation } = { kind };
    const result = await mutationCapture.run(capture, execute);
    if (!capture.fileChange || result?.isError) return result;
    const fileChange = kind === "edit"
      ? retainAppliedPatch(capture.fileChange, result?.details?.patch)
      : capture.fileChange;
    return { ...result, details: { ...result?.details, fileChange } };
  };

  const mkdir = async (dirPath: string) => {
    const absolute = filePathFromRefPath(dirPath, cwd);
    const stat = await resourceIO.stat(refForPath(absolute));
    if (stat.exists) return;
    await resourceIO.mkdir(refForPath(absolute), operationContext("agent_mkdir", { emit: false }));
  };

  return {
    read: {
      readFile,
      access: (filePath: string) => access(filePath, constants.R_OK),
      detectImageMimeType: detectImageMimeType
        ? async (filePath: string) => detectImageMimeType(filePath)
        : undefined,
    },
    write: {
      writeFile,
      mkdir,
    },
    edit: {
      readFile,
      writeFile: editWriteFile,
      access: async (filePath: string) => {
        const stat = await resourceIO.stat(refForPath(filePath));
        if (!stat.exists) throw new Error(`Path not found: ${filePath}`);
      },
    },
    ls: {
      exists: async (filePath: string) => (await resourceIO.stat(refForPath(filePath))).exists,
      stat: async (filePath: string) => statLike(await resourceIO.stat(refForPath(filePath))),
      readdir: async (dirPath: string) => {
        const result = await resourceIO.list(refForPath(dirPath));
        return result.items.map((item) => item.name);
      },
    },
    grep: {
      isDirectory: async (filePath: string) => (await resourceIO.stat(refForPath(filePath))).isDirectory,
      readFile: async (filePath: string) => (await resourceIO.read(refForPath(filePath))).content.toString("utf-8"),
    },
    find: {
      exists: async (filePath: string) => (await resourceIO.stat(refForPath(filePath))).exists,
    },
    withResourceTarget,
    hasBoundTarget,
    withFileChangeCapture,
  };
}
