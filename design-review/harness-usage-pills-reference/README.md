# harness-usage-pills-reference

移植参考包：DeepSeek Harness（github.com/deepseek-ai/deepseek-harness，MIT 许可）
聊天界面「用量/用时」胶囊的原版实现，2026-09-05 取自 master 分支。

- TurnUsagePanel.tsx —— TurnUsagePanel（用量胶囊+弹窗）与 TurnTimePanel（用时胶囊+弹窗）组件
- TurnUsagePanel.module.css —— 两个胶囊与弹窗的全部样式
- token-format.ts —— formatTokens（紧凑 K/M）/ formatExactTokens（千分位）/ formatCacheHitPercent（命中百分比，防 99.9→100 失真）
- message-chrome.ts —— formatRunDuration / formatTokensPerSecond / formatLatencySeconds
- locale-keys.zh.txt —— 弹窗中文文案键

移植惯例参考本项目 desktop/src/react/settings/tabs/observability/trace-detail/trajectory-record.ts 的文件头写法。
