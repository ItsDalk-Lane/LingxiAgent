// stdout专用于正式JSONL；SDK自己的日志不能污染协议或泄漏凭据。
console.log = console.info = console.warn = console.error = () => {};
try {
  const { runBridgeSdkWorker } = await import("../lib/bridge/sdk-worker.ts");
  await runBridgeSdkWorker(process.stdin, process.stdout);
  process.exit(0);
} catch {
  process.stderr.write("bridge_sdk_worker_failed\n");
  process.exit(1);
}
