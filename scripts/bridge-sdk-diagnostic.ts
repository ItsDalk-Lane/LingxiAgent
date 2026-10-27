// 诊断同样只向 stdout 输出正式结果，SDK 日志不能混入凭据响应。
console.log = console.info = console.warn = console.error = () => {};
const abort = new AbortController();
process.on("SIGINT", () => abort.abort());
process.on("SIGTERM", () => abort.abort());
try {
  const { runBridgeSdkDiagnostic } = await import("../lib/bridge/sdk-diagnostic.ts");
  await runBridgeSdkDiagnostic(process.stdin, process.stdout, abort.signal);
  process.exit(0);
} catch {
  process.stderr.write("bridge_sdk_diagnostic_failed\n");
  process.exit(1);
}
