# 本地模型子系统测试报告

> 本文保留 Phase 5 的历史结果，不代表当前源码的最后一轮门禁。Phase 6 四类真实模型、内存/性能失败项和最新回归见 [本机真实验证报告](LOCAL_MODELS_REAL_VALIDATION_REPORT.md)。

日期：2026-09-02  
环境：macOS arm64，Node.js `v24.16.0`，npm `11.13.0`

## 总结

| 检查 | 状态 | 证据 |
|---|---|---|
| 全量 Vitest | PASS | 1,292 个文件通过、1 个跳过；13,033 项通过、7 项跳过；最终轮 82.65 秒 |
| 三段 TypeScript | PASS | 根、Node、测试配置均退出 0 |
| 本任务范围代码规范 | PASS | 新增/修改范围 0 error；最近构建脚本有 1 条既有空块 warning |
| 全仓 ESLint | BOUNDED FAIL | 仅未修改的 `lib/security/injection-scan.ts:24:31` 有 1 条既有规则错误 |
| 开放边界 | PASS | 1 条既有受控债务，0 条新增临时豁免 |
| 持久化指纹门禁 | PASS | 受保护源码与新指纹同批更新 |
| 差异格式 | PASS | `git diff --check` 退出 0 |
| packages/client 构建 | PASS | 工作区包、主进程、预加载、渲染器、启动页、主题均构建成功 |
| 独立 server 构建 | PASS | 原生依赖安装、裁剪和四项真实加载 smoke 通过 |
| 种子包复核 | PASS | 一次性测试 keyset 下清单及签名验证通过 |
| Electron 目录打包 | PASS | `dist/mac-arm64/Lingxi.app` 生成并通过深度签名校验 |
| 依赖审计 | BOUNDED FAIL | `npm audit --omit=dev`：7 个 moderate，均为既有传递链；强修会造成破坏性降级 |
| 真实模型性能 | NOT_EXECUTED | 没有已审核权重和运行时资产 |
| Windows/macOS Intel/Linux | NOT_EXECUTED | 当前只有 macOS arm64 本机环境 |

## 自动化覆盖

专项测试覆盖：

- 清单解析、版本兼容、损坏拒绝、缓存与回退。
- 断点续传、暂停、取消、镜像、代理、空间不足和摘要失败。
- 安全解压的路径逃逸、符号链接、未声明文件、重名和压缩炸弹。
- 有/无元数据手动导入、源目录只读、候选确认和运行时复核。
- 引用计数、并发冷加载、热复用、空闲卸载、卸载竞态和取消。
- 大模型单槽位、排队取消、small/large 并行和内存拒绝。
- 子进程握手、随机令牌、取消、超时、崩溃、退出清理和脱敏日志。
- 本地嵌入、图片/PDF 文字识别、桌面/Bridge 语音识别、朗读和 Bridge 语音合成的业务合同。
- 路由权限、设置页下载管理、导入确认、删除确认、许可证和五语言。
- Electron 收集规则、服务器外置依赖、画布加载 smoke、零外网源码边界。

## 生产构建证据

服务器目标 Node 实际通过：

- 内存数据库打开与查询。
- 中文分词器加载及自定义词典。
- 文档转换原生绑定加载并完成转换。
- 原生画布创建、绘制并编码 PNG。

应用目录通过 `codesign --verify --deep --strict`。应用文件树和内嵌 server 归档均没有 `.gguf`、`.onnx`、`.safetensors`、`.ggml` 或 `.ort` 文件，也没有八模型安装目录。

## 基准工具

权威入口为 `scripts/bench/local-models.mts`，npm 命令为 `npm run benchmark:local-models`。在一个空的临时 `LINGXI_HOME` 下实跑退出 0，四类结果分别为：

- embedding：`NOT_EXECUTED`，没有已安装模型。
- OCR：`NOT_EXECUTED`，没有输入图片。
- STT：`NOT_EXECUTED`，没有输入音频。
- TTS：`NOT_EXECUTED`，没有已安装模型。

这次只证明工具会诚实区分“没跑”和“通过”，不构成性能证据。

## 受限失败说明

全仓代码规范失败不来自本任务文件；为了遵守精准修改原则，没有顺手修改该安全扫描文件。依赖审计的 7 个中危来自 `request/uuid` 传递链，涉及 Telegram 与表格依赖；自动强修会把表格依赖降到不兼容主版本，因此本任务没有执行破坏性升级。

## 未执行矩阵

- 八个真实模型的下载、恢复、完整性与推理：`NOT_EXECUTED`。
- 真实知识向量、真实扫描文档、真实离线录音和真实离线语音合成：`NOT_EXECUTED`。
- 4 核 8GB、8 核 x86、Apple M 系列性能阈值：`NOT_EXECUTED`。
- win-x64、darwin-x64、linux-x64/arm64 原生加载和安装包：`NOT_EXECUTED`。
- Apple Developer ID、正式公证、GitHub Release 与自动更新：`NOT_EXECUTED`。
- 设置页真人视觉验收和任务管理器/活动监视器内存观测：`NOT_EXECUTED`。
