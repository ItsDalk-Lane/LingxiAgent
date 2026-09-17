import { getToolSessionPath } from "../tools/tool-session.ts";
import { randomBytes } from "node:crypto";
import { createWriteStream, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EXEC_COMMAND_MAX_TIMEOUT_SECONDS,
  extractExitCode,
  firstText,
  jsonResult,
  mergeExecDetails,
  textResult,
} from "./schema.ts";

const DEFAULT_MAX_LINES = 2000;
const DEFAULT_MAX_BYTES = 50 * 1024;
const MAX_ROLLING_BYTES = DEFAULT_MAX_BYTES * 2;

function truncateText(text: string, maxOutputTokens: number) {
  const maxChars = Math.max(1000, Math.floor(maxOutputTokens * 4));
  if (String(text || "").length <= maxChars) return text;
  return String(text).slice(0, maxChars) + "\n\n[exec_command output truncated]";
}

function normalizeThrownToolError(err: any, maxOutputTokens: number) {
  if (err?.hanaCommandBlockedResult) {
    return firstText(err.hanaCommandBlockedResult);
  }
  const text = err?.message || String(err);
  return truncateText(text, maxOutputTokens);
}

function getTempFilePath() {
  const id = randomBytes(8).toString("hex");
  return join(tmpdir(), `hana-exec-command-${id}.log`);
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

function truncateStringToBytesFromEnd(str: string, maxBytes: number) {
  const buf = Buffer.from(str, "utf-8");
  if (buf.length <= maxBytes) return str;
  let start = buf.length - maxBytes;
  while (start < buf.length && (buf[start] & 0xc0) === 0x80) start++;
  return buf.slice(start).toString("utf-8");
}

function truncateTail(content: string, {
  maxLines = DEFAULT_MAX_LINES,
  maxBytes = DEFAULT_MAX_BYTES,
} = {}) {
  const totalBytes = Buffer.byteLength(content, "utf-8");
  const lines = content.split("\n");
  const totalLines = lines.length;
  if (totalLines <= maxLines && totalBytes <= maxBytes) {
    return {
      content,
      truncated: false,
      truncatedBy: null,
      totalLines,
      totalBytes,
      outputLines: totalLines,
      outputBytes: totalBytes,
      lastLinePartial: false,
      maxLines,
      maxBytes,
    };
  }

  const outputLinesArr: string[] = [];
  let outputBytesCount = 0;
  let truncatedBy = "lines";
  let lastLinePartial = false;
  for (let i = lines.length - 1; i >= 0 && outputLinesArr.length < maxLines; i--) {
    const line = lines[i];
    const lineBytes = Buffer.byteLength(line, "utf-8") + (outputLinesArr.length > 0 ? 1 : 0);
    if (outputBytesCount + lineBytes > maxBytes) {
      truncatedBy = "bytes";
      if (outputLinesArr.length === 0) {
        const truncatedLine = truncateStringToBytesFromEnd(line, maxBytes);
        outputLinesArr.unshift(truncatedLine);
        outputBytesCount = Buffer.byteLength(truncatedLine, "utf-8");
        lastLinePartial = true;
      }
      break;
    }
    outputLinesArr.unshift(line);
    outputBytesCount += lineBytes;
  }
  if (outputLinesArr.length >= maxLines && outputBytesCount <= maxBytes) {
    truncatedBy = "lines";
  }
  const outputContent = outputLinesArr.join("\n");
  return {
    content: outputContent,
    truncated: true,
    truncatedBy,
    totalLines,
    totalBytes,
    outputLines: outputLinesArr.length,
    outputBytes: Buffer.byteLength(outputContent, "utf-8"),
    lastLinePartial,
    maxLines,
    maxBytes,
  };
}

// 保头保尾：开头常有启动/报错头，结尾常有结果摘要，两头都要；中间精确标记省略量。
export function truncateHeadTail(content: string, {
  maxLines = DEFAULT_MAX_LINES,
  maxBytes = DEFAULT_MAX_BYTES,
} = {}) {
  const totalBytes = Buffer.byteLength(content, "utf-8");
  const lines = content.split("\n");
  const totalLines = lines.length;
  if (totalLines <= maxLines && totalBytes <= maxBytes) {
    return { ...truncateTail(content, { maxLines, maxBytes }) };
  }

  const headLineBudget = Math.max(1, Math.floor(maxLines / 2));
  const tailLineBudget = Math.max(1, maxLines - headLineBudget);
  const headByteBudget = Math.floor(maxBytes / 2);
  const tailByteBudget = maxBytes - headByteBudget;

  const headLines: string[] = [];
  let headBytes = 0;
  for (let i = 0; i < lines.length && headLines.length < headLineBudget; i++) {
    const lineBytes = Buffer.byteLength(lines[i], "utf-8") + (headLines.length > 0 ? 1 : 0);
    if (headBytes + lineBytes > headByteBudget) break;
    headLines.push(lines[i]);
    headBytes += lineBytes;
  }

  const tailLines: string[] = [];
  let tailBytes = 0;
  for (let i = lines.length - 1; i >= 0 && tailLines.length < tailLineBudget; i--) {
    if (i < headLines.length) break;
    const lineBytes = Buffer.byteLength(lines[i], "utf-8") + (tailLines.length > 0 ? 1 : 0);
    if (tailBytes + lineBytes > tailByteBudget) break;
    tailLines.unshift(lines[i]);
    tailBytes += lineBytes;
  }

  const headEnd = headLines.length;
  const tailStart = lines.length - tailLines.length;
  if (tailStart <= headEnd) {
    // 两段相遇说明总量只超一点点：退化为按字节的头尾切分。
    const headPart = truncateStringToBytesFromStart(content, headByteBudget);
    const tailPart = truncateStringToBytesFromEnd(content, tailByteBudget);
    if (headPart.length + tailPart.length >= content.length) {
      return { ...truncateTail(content, { maxLines, maxBytes }) };
    }
    const omittedBytes = totalBytes - Buffer.byteLength(headPart, "utf-8") - Buffer.byteLength(tailPart, "utf-8");
    const outputContent = `${headPart}\n[... ${formatSize(omittedBytes)} omitted ...]\n${tailPart}`;
    return {
      content: outputContent,
      truncated: true,
      truncatedBy: "head_tail",
      totalLines,
      totalBytes,
      outputLines: headPart.split("\n").length + tailPart.split("\n").length,
      outputBytes: Buffer.byteLength(outputContent, "utf-8"),
      headLines: headPart.split("\n").length,
      tailLines: tailPart.split("\n").length,
      omittedLines: 0,
      omittedBytes,
      lastLinePartial: false,
      maxLines,
      maxBytes,
    };
  }

  const omittedLines = tailStart - headEnd;
  const omittedLinesContent = lines.slice(headEnd, tailStart).join("\n");
  const omittedBytes = Buffer.byteLength(omittedLinesContent, "utf-8");
  const outputContent = `${headLines.join("\n")}\n[... ${omittedLines} lines / ${formatSize(omittedBytes)} omitted ...]\n${tailLines.join("\n")}`;
  return {
    content: outputContent,
    truncated: true,
    truncatedBy: "head_tail",
    totalLines,
    totalBytes,
    outputLines: headLines.length + tailLines.length,
    outputBytes: Buffer.byteLength(outputContent, "utf-8"),
    headLines: headLines.length,
    tailLines: tailLines.length,
    omittedLines,
    omittedBytes,
    lastLinePartial: false,
    maxLines,
    maxBytes,
  };
}

function truncateStringToBytesFromStart(str: string, maxBytes: number) {
  const buf = Buffer.from(str, "utf-8");
  if (buf.length <= maxBytes) return str;
  let end = maxBytes;
  while (end > 0 && (buf[end] & 0xc0) === 0x80) end--;
  return buf.slice(0, end).toString("utf-8");
}

// 超时提示固定带下一步动作（默认超时注明、教长任务用 timeout/tty 续接）。
function formatTimeoutNotice(timeoutSecs: string | number, { defaulted = false }: { defaulted?: boolean } = {}) {
  const base = `Command timed out after ${timeoutSecs} seconds${defaulted ? " (default timeout)" : ""}`;
  const hint = `For long-running work pass timeout=<seconds> (max ${EXEC_COMMAND_MAX_TIMEOUT_SECONDS}), or run with tty=true and continue via write_stdin.`;
  return `${base}. ${hint}`;
}

function isValidUtf8(buffer: Buffer) {
  for (let i = 0; i < buffer.length;) {
    const byte = buffer[i];
    if (byte <= 0x7f) {
      i++;
      continue;
    }

    let needed = 0;
    let min = 0;
    let codePoint = 0;
    if (byte >= 0xc2 && byte <= 0xdf) {
      needed = 1;
      min = 0x80;
      codePoint = byte & 0x1f;
    } else if (byte >= 0xe0 && byte <= 0xef) {
      needed = 2;
      min = 0x800;
      codePoint = byte & 0x0f;
    } else if (byte >= 0xf0 && byte <= 0xf4) {
      needed = 3;
      min = 0x10000;
      codePoint = byte & 0x07;
    } else {
      return false;
    }

    if (i + needed >= buffer.length) return false;
    for (let j = 1; j <= needed; j++) {
      const next = buffer[i + j];
      if ((next & 0xc0) !== 0x80) return false;
      codePoint = (codePoint << 6) | (next & 0x3f);
    }
    if (codePoint < min || codePoint > 0x10ffff) return false;
    if (codePoint >= 0xd800 && codePoint <= 0xdfff) return false;
    i += needed + 1;
  }
  return true;
}

export function decodeCommandOutput(buffer: Buffer, {
  platform = process.platform,
}: { platform?: NodeJS.Platform } = {}) {
  if (!buffer.length) {
    return { text: "", encoding: "utf-8", transcoded: false };
  }
  if (platform === "win32" && !isValidUtf8(buffer)) {
    try {
      return {
        text: new TextDecoder("gbk").decode(buffer),
        encoding: "gbk",
        transcoded: true,
      };
    } catch {}
  }
  return {
    text: buffer.toString("utf-8"),
    encoding: "utf-8",
    transcoded: false,
  };
}

class CommandOutputCollector {
  private readonly platform: NodeJS.Platform;
  private chunks: Buffer[] = [];
  private chunksBytes = 0;
  private totalBytes = 0;
  private tempFilePath: string | undefined;
  private tempFileStream: any;

  constructor(platform: NodeJS.Platform) {
    this.platform = platform;
  }

  append(data: Buffer) {
    const chunk = Buffer.isBuffer(data) ? data : Buffer.from(data);
    this.totalBytes += chunk.length;
    if (this.totalBytes > DEFAULT_MAX_BYTES) {
      this.ensureTempFile();
    }
    if (this.tempFileStream) this.tempFileStream.write(chunk);

    this.chunks.push(chunk);
    this.chunksBytes += chunk.length;
    while (this.chunksBytes > MAX_ROLLING_BYTES && this.chunks.length > 1) {
      const removed = this.chunks.shift();
      this.chunksBytes -= removed?.length || 0;
    }
  }

  snapshot() {
    const buffer = Buffer.concat(this.chunks);
    return decodeCommandOutput(buffer, { platform: this.platform });
  }

  close() {
    if (this.tempFileStream) {
      this.tempFileStream.end();
      this.tempFileStream = undefined;
    }
  }

  private ensureTempFile() {
    if (this.tempFilePath) return;
    this.tempFilePath = getTempFilePath();
    this.tempFileStream = createWriteStream(this.tempFilePath);
    for (const chunk of this.chunks) this.tempFileStream.write(chunk);
  }

  get fullOutputPath() {
    return this.tempFilePath;
  }
}

function buildFinalCommandResult(output: string, exitCode: number | null, {
  fullOutputPath,
  encoding,
  transcoded,
}: {
  fullOutputPath?: string;
  encoding?: string;
  transcoded?: boolean;
}) {
  const truncation = truncateHeadTail(output);
  let outputText = truncation.content || "(no output)";
  let outputPath = fullOutputPath;
  const details: Record<string, any> = {};

  if (truncation.truncated) {
    details.truncation = truncation;
    if (!outputPath) {
      try {
        outputPath = getTempFilePath();
        writeFileSync(outputPath, output, "utf-8");
      } catch {
        outputPath = undefined;
      }
    }
    if (outputPath) details.fullOutputPath = outputPath;
    const fullOutputNotice = outputPath ? `. Full output: ${outputPath}` : "";
    if (truncation.truncatedBy === "head_tail") {
      const headLabel = truncation.omittedLines > 0
        ? `Showing first ${truncation.headLines} and last ${truncation.tailLines} of ${truncation.totalLines} lines`
        : `Showing ${truncation.headLines + truncation.tailLines} of ${truncation.totalLines} lines`;
      outputText += `\n\n[${headLabel}${fullOutputNotice}]`;
    } else if (truncation.lastLinePartial) {
      const lastLineSize = formatSize(Buffer.byteLength(output.split("\n").pop() || "", "utf-8"));
      outputText += `\n\n[Showing last ${formatSize(truncation.outputBytes)} of line ${truncation.totalLines} (line is ${lastLineSize})${fullOutputNotice}]`;
    } else if (truncation.truncatedBy === "lines") {
      const startLine = truncation.totalLines - truncation.outputLines + 1;
      const endLine = truncation.totalLines;
      outputText += `\n\n[Showing lines ${startLine}-${endLine} of ${truncation.totalLines}${fullOutputNotice}]`;
    } else {
      const startLine = truncation.totalLines - truncation.outputLines + 1;
      const endLine = truncation.totalLines;
      outputText += `\n\n[Showing lines ${startLine}-${endLine} of ${truncation.totalLines} (${formatSize(DEFAULT_MAX_BYTES)} limit)${fullOutputNotice}]`;
    }
  } else if (outputPath) {
    details.fullOutputPath = outputPath;
  }

  if (exitCode !== 0 && exitCode !== null) {
    outputText += `\n\nCommand exited with code ${exitCode}`;
  }
  if (encoding) {
    details.outputEncoding = encoding;
    details.outputTranscoded = !!transcoded;
  }
  return textResult(outputText, details);
}

export async function runExecCommandOnce({
  bashTool,
  toolCallId,
  command,
  timeout,
  timeoutDefaulted = false,
  signal,
  onUpdate,
  ctx,
  execDetails,
  maxOutputTokens,
}: any) {
  try {
    const params: any = { command };
    if (timeout) params.timeout = timeout;
    const result = await bashTool.execute(toolCallId, params, signal, onUpdate, ctx);
    const text = firstText(result);
    const exitCode = extractExitCode(text) ?? 0;
    return mergeExecDetails(result, {
      ...execDetails,
      ok: exitCode === 0,
      exitCode,
      transportError: false,
      ...(exitCode !== 0 ? {
        // 命令跑完但失败：正常输出交模型判断；依赖探测失败才升级为错误。
        isError: execDetails?.classification?.kind === "probe",
        errorCode: execDetails?.classification?.kind === "probe"
          ? "EXEC_COMMAND_DEPENDENCY_MISSING"
          : "EXEC_COMMAND_EXIT_NONZERO",
      } : {}),
    });
  } catch (err) {
    const output = normalizeThrownToolError(err, maxOutputTokens);
    const exitCode = extractExitCode(output);
    const isTimeout = /Command timed out after \d+ seconds/.test(output);
    const isAbort = /Command aborted/.test(output);
    let finalOutput = output;
    if (isTimeout && !/For long-running work pass timeout=/.test(output)) {
      const timeoutSecs = output.match(/Command timed out after (\d+) seconds/)?.[1] ?? String(timeout ?? "");
      finalOutput = output.replace(
        /Command timed out after \d+ seconds/,
        formatTimeoutNotice(timeoutSecs, { defaulted: timeoutDefaulted === true }),
      );
    }
    // 非零退出与超时是命令的正常结局；中止、依赖探测失败、传输层故障才是工具错误。
    // 注意不要经 textResult 传 errorCode——那里会因 errorCode 自动标 isError。
    const isError = isAbort
      || (execDetails?.classification?.kind === "probe" && !isTimeout)
      || (exitCode === null && !isTimeout);
    return mergeExecDetails(textResult(finalOutput), {
      ...execDetails,
      ok: false,
      exitCode,
      transportError: false,
      isError,
      errorCode: isTimeout
        ? "EXEC_COMMAND_TIMEOUT"
        : execDetails?.classification?.kind === "probe"
          ? "EXEC_COMMAND_DEPENDENCY_MISSING"
          : "EXEC_COMMAND_EXIT_NONZERO",
    });
  }
}

export async function runExecCommandDirect({
  commandExec,
  command,
  workdir,
  timeout,
  timeoutDefaulted = false,
  signal,
  onUpdate,
  execDetails,
  maxOutputTokens,
  platform = process.platform,
}: any) {
  const collector = new CommandOutputCollector(platform);

  try {
    if (onUpdate) onUpdate({ content: [], details: undefined });
    const result = await commandExec(command, workdir, {
      timeout,
      signal,
      onData: (data: Buffer) => {
        collector.append(data);
        if (!onUpdate) return;
        const decoded = collector.snapshot();
        const truncation = truncateTail(decoded.text);
        onUpdate({
          content: [{ type: "text", text: truncation.content || "" }],
          details: {
            truncation: truncation.truncated ? truncation : undefined,
            fullOutputPath: collector.fullOutputPath,
            outputEncoding: decoded.encoding,
            outputTranscoded: decoded.transcoded,
          },
        });
      },
    });
    collector.close();
    const decoded = collector.snapshot();
    const exitCode = result?.exitCode ?? 0;
    const toolResult = buildFinalCommandResult(decoded.text, exitCode, {
      fullOutputPath: collector.fullOutputPath,
      encoding: decoded.encoding,
      transcoded: decoded.transcoded,
    });
    return mergeExecDetails(toolResult, {
      ...execDetails,
      ok: exitCode === 0,
      exitCode,
      transportError: false,
      ...(exitCode !== 0 ? {
        // 命令跑完但失败：正常输出交模型判断；依赖探测失败才升级为错误。
        isError: execDetails?.classification?.kind === "probe",
        errorCode: execDetails?.classification?.kind === "probe"
          ? "EXEC_COMMAND_DEPENDENCY_MISSING"
          : "EXEC_COMMAND_EXIT_NONZERO",
      } : {}),
    });
  } catch (err) {
    collector.close();
    if (err?.hanaCommandBlockedResult) {
      return mergeExecDetails(err.hanaCommandBlockedResult, {
        ...execDetails,
        ok: false,
        transportError: false,
        isError: true,
        errorCode: "EXEC_COMMAND_BLOCKED",
      });
    }

    const decoded = collector.snapshot();
    let output = decoded.text;
    let isTimeout = false;
    let isAbort = false;
    if (err?.message === "aborted") {
      isAbort = true;
      if (output) output += "\n\n";
      output += "Command aborted";
    } else if (typeof err?.message === "string" && err.message.startsWith("timeout:")) {
      isTimeout = true;
      if (output) output += "\n\n";
      output += formatTimeoutNotice(err.message.split(":")[1], { defaulted: timeoutDefaulted === true });
    } else {
      if (output) output += "\n\n";
      output += err?.message || String(err);
    }

    const exitCode = extractExitCode(output);
    const toolResult = buildFinalCommandResult(
      truncateText(output, maxOutputTokens),
      exitCode,
      {
        fullOutputPath: collector.fullOutputPath,
        encoding: decoded.encoding,
        transcoded: decoded.transcoded,
      },
    );
    return mergeExecDetails(toolResult, {
      ...execDetails,
      ok: false,
      exitCode,
      transportError: false,
      isError: isTimeout ? false : isAbort ? true : exitCode !== null
        ? execDetails?.classification?.kind === "probe"
        : true,
      errorCode: isTimeout
        ? "EXEC_COMMAND_TIMEOUT"
        : execDetails?.classification?.kind === "probe"
          ? "EXEC_COMMAND_DEPENDENCY_MISSING"
          : "EXEC_COMMAND_EXIT_NONZERO",
    });
  }
}

export async function startExecCommandTty({
  toolCallId,
  manager,
  getAgentId,
  getCwd,
  command,
  workdir,
  label,
  ctx,
  execDetails,
  cols = 80,
  rows = 24,
}: any) {
  const sessionPath = getToolSessionPath(ctx);
  if (!sessionPath) {
    return textResult("current session is required to start an interactive command", {
      errorCode: "EXEC_COMMAND_SESSION_REQUIRED",
      execCommand: execDetails,
    });
  }
  if (!manager) {
    return textResult("terminal manager unavailable", {
      errorCode: "EXEC_COMMAND_TERMINAL_MANAGER_UNAVAILABLE",
      execCommand: execDetails,
    });
  }
  const result = await manager.start({
    toolCallId,
    sessionPath,
    agentId: getAgentId?.() || "",
    cwd: workdir || ctx?.sessionManager?.getCwd?.() || getCwd?.() || process.cwd(),
    command,
    label: label || "exec_command",
    cols,
    rows,
  });
  return jsonResult({
    ...result,
    processId: result.terminalId,
    process_id: result.terminalId,
    execCommand: {
      ...execDetails,
      ok: true,
      processId: result.terminalId,
      terminalId: result.terminalId,
      transportError: false,
    },
  });
}
