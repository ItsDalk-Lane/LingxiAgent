/** 等待系统通知的实际展示事件；提交请求不能当作送达回执。 */
function showNotificationWithReceipt(notification, { timeoutMs = 10_000 } = {}) {
  return new Promise((resolve) => {
    let settled = false;
    let timer;
    const finish = (receipt) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      notification.removeListener("show", shown);
      notification.removeListener("failed", failed);
      notification.removeListener("close", closed);
      resolve(receipt);
    };
    const shown = () => finish({ shown: true });
    const failed = () => finish({ shown: false, reason: "failed" });
    const closed = () => finish({ shown: false, reason: "unconfirmed" });
    notification.once("show", shown);
    notification.once("failed", failed);
    notification.once("close", closed);
    timer = setTimeout(() => finish({ shown: false, reason: "unconfirmed" }), timeoutMs);
    timer.unref?.();
    try {
      notification.show();
    } catch {
      // 调用可能已经跨过系统边界，不能猜测为一定未展示。
      finish({ shown: false, reason: "unconfirmed" });
    }
  });
}
module.exports = { showNotificationWithReceipt };
