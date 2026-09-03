# 本地模型：本机真实验证报告（Phase 6）

> 本文为首轮实测快照。三项问题后续修复及未解决边界见 [Phase 7 修复复验](LOCAL_MODELS_FIX_VERIFICATION.md)；保留本文原始失败数字用于对照。

日期：2026-09-02。状态：四类首轮本机真实验证已完成，发现未通过项；不是全部产品验收通过。

## 结论与范围

四类能力均已使用真实权重运行：嵌入、图片/扫描 PDF 识字、语音转文字、文字合成语音。它们也在进程级禁止外网的条件下执行成功，不依赖云端返回或模拟输出。

尚不能宣布产品验收通过：语音内存没有按要求回落；长句语音合成取消超过 1 秒；纯 CPU 识字内存和耗时明显超标。四类各一个模型不等于原任务全部模型、精度和平台均验收完毕。桌面麦克风、完整朗读交互与真实 Bridge 平台仍未实测。

本阶段依据用户允许下载的授权，在原工作区补齐实际运行适配并验证；没有提交、推送、部署、正式签名、公证或发布，没有改日常模型配置或操作真实知识库/会话。

## 环境与证据位置

- 源码工作区：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `feat/pending-sep02`，HEAD `3eab85891a1747c64064252804f70c0a3773f021`；包含未提交改动，不应按 HEAD 纯净源码复现。
- 本机：Apple M3 Ultra，96 GiB 内存，28 核；macOS arm64；Node v24.16.0；本轮模型设置为 4 线程。
- 全部模型、运行包、隔离数据和原始报告：`/Users/study_superior/Downloads/lingxi-local-validation.w4b2l4`。下文简称“证据根目录”，不是日常应用数据目录；约占 7.9 GiB，含下载包、导入副本、测试结果及失败下载残包，暂保留以便复核。
- `results/` 保留成功和失败轮次；每次真实调用保存输出。`verify.mjs`、`lifecycle.mjs`、`knowledge.mjs`、`scheduling.mjs` 是本机复跑脚本，导入当前工作区的生产子系统。单元测试没有在运行时下载权重。
- [证据索引](LOCAL_MODELS_REAL_EVIDENCE_INDEX.json) 保存 130 份原始文件的路径、字节数与 SHA-256，以及四个导入模型的完整元数据和适配源码摘要；约 21.5 MB 小型证据，权重本体不复制入仓库。索引不是签名审计封印，也不代表已提交源码。
- 合成图片明确加载系统中文字体；扫描 PDF 仅有一张图片，没有文字层。PDF 技能的渲染回看发现并排除了最初缺字字体和旧色块夹具，不将那一轮计入文字准确性证据。生成夹具所用 Python 不属于产品运行依赖。

## 实际运行模型

| 能力 | 实际模型与精度 | 本机执行方式 | 事实边界 |
|---|---|---|---|
| 嵌入 | Qwen3-Embedding-0.6B，Q8_0，1024 维 | llama.cpp b10621，Metal / 强制 CPU | 不是 fp8；实测原生进程超过 1 GiB，因此隔离元数据按大档登记 |
| 识字 | GLM-OCR，由 f16 真正量化为 Q4_K_M；视觉投影 Q8_0 | llama.cpp b10621，Metal / 强制 CPU | 不把投影冒称 q4；不代表 PaddleOCR-VL 已验证 |
| 语音转文字 | SenseVoice Small，int8 ONNX | sherpa-onnx-node 1.13.7，进程内工作线程、CPU | 只验证 WAV；桌面与 Bridge 的其他录音格式未闭环 |
| 文字朗读 | Kokoro 多语言 v1.0，fp32 ONNX | sherpa-onnx-node 1.13.7，进程内工作线程、CPU | 两个中文音色；输出完整 WAV，不是原生音频分片实时流 |

受管身份分别为：

- `local:qwen3-embedding-0.6b@q8@manual-5adb54bee676`
- `local:glm-ocr@q4@manual-72a01d1171e1`
- `local:sensevoice-small@int8@manual-e8755e4372da`
- `local:kokoro-82m@fp32@manual-b106f0fc1ba3`

手动导入的来源类别仍是 `manual`、完整性标签仍是 `unknown`，没有伪装为产品远程清单已经发布。独立下载摘要核验是本次额外证据。

## 四类真实业务证据

### 嵌入与知识检索

- `results/embedding-metal-1788362251783/report.json`：1024 维；相近两句余弦相似度 0.8005，无关句 0.1350。
- `results/knowledge-1788362546971/report.json`：真正导入 Markdown、TXT、CSV，经过解析、分块和向量入库，三个向量变体均 ready；三问均命中预期文件，包括用中文查英文的远程办公政策。共六次真实嵌入调用，不是直接返回固定命中文件。
- `results/embedding-cpu-1788362317254/report.json`：强制 CPU 成功；`results/embedding-auto-1788362456629/report.json`：禁止外网下自动选择 Metal 成功。
- 未验证另一精度/模型切换后新旧向量共同保留的真实权重场景，F9 只能算部分通过。

### 图片与扫描文档识字

- 正式采用 `results/ocr-auto-1788363473548/report.json`，五项内容均命中：中文标题、2026 年 9 月 2 日、12800 元、张明、英文测试行。
- 真实文档抽取入口分别读取 PNG 和没有文字层的扫描 PDF；二者返回识字结果及识字来源标记，未伪装成视觉理解。
- `results/ocr-auto-1788363547546/report.json` 在进程级断外网后重复通过。
- 首轮 `ocr-auto-1788363040651` 的脚本 PASS 仅说明当时较弱断言通过，输入本身缺字；**不得作为中文准确率证明**。本轮只覆盖清晰印刷文字，不代表表格、手写、倾斜、多栏、超长文档质量达标。

### 两项语音

- 官方 5.592 秒中文音频得到实际转写：“开饭时间早上9点至下午5点。”保留原始识别结果，不把它修写成参考答案，不宣称零错字。业务服务在隔离会话中返回 ready 和中文语言标记。
- Kokoro 合成约 4.203 秒、24 kHz 的真实 WAV；业务服务使用第二个中文音色也输出 WAV。文件在各 TTS 结果目录中，未发送外部平台。
- 最新安全取消实现的证据：`results/lifecycle-stt-1788362959042/report.json`、`results/lifecycle-tts-1788362727996/report.json`。取消后再次调用成功；合成仍需 1569.6 ms 才结束，不能算 1 秒内停止通过。
- TTS 最新断网功能复测：`results/tts-cpu-1788362866476/report.json`；该轮与全量测试同时运行，有竞争负载，速度不作为无竞争基线。
- 四能力同时请求后，将 Kokoro 真正生成的 WAV 再交给 SenseVoice，得到“这是本地语音测试。”，与输入一致。这是机器回转验证，不代替真人听感评价。

## 性能与内存：实测口径

首次耗时包含检查、后端探测、加载和首次推理，**不是单独冷加载时长**。热调用为小样本，不是分位数基准。独立脚本每 100 ms 采样宿主和子进程树；内存单位为 MiB，不能把进程树总量当作模型独占内存或显存。当前产品诊断中名为 `peakRssMb` 的字段实际只在完成时读取一次，不是真正峰值，不能拿来宣称 F13 已达标。

| 模型/模式 | 首次端到端 | 热调用 | 独立进程树峰值 → 释放后 | 说明 |
|---|---:|---:|---:|---|
| Qwen Q8 / Metal | 3268 ms | 34.5 / 33.7 ms | 1553 → 271 MiB | 基线 257 MiB；后续离线首次 911 ms，已有系统缓存 |
| Qwen Q8 / CPU | 1541 ms | 65.7 / 65.3 ms | 1942 → 216 MiB | 基线 198 MiB；原生进程约 1635 MiB |
| GLM Q4 / Metal | 1855 ms | 195 / 183 ms | 1938 → 305 MiB | 基线 188 MiB；重复同图命中缓存，不代表每张新图速度；扫描页真实推理 3257 ms |
| GLM Q4 / CPU | 22331 ms | 366 / 365 ms | 9275 → 536 MiB | 基线 183 MiB；同图缓存热调用；新扫描页 110545 ms，未满足速度/内存目标 |
| SenseVoice / CPU | 1177 ms | 74.5 / 75.1 ms | 994 → 963 MiB | 早期功能轮；5.592 秒音频；卸载后内存不回落，后续安全版重复加载仍复现 |
| Kokoro / CPU | 1858 ms | 1181 / 1179 ms | 1020 → 1020 MiB | 早期功能轮；生成 4.203 秒音频，耗时比例约 0.28，超过 0.15 目标 |

系统断网沙箱禁止调用系统进程监测工具时，报告明确标记 `parent-only`，省略缺失的原生内存值；该轮“进程树峰值”只含宿主，不能用于模型内存验收，也不能将缺测记作零。

### 已证实的问题，不做通过包装

1. **语音卸载不达标。** 安全版 SenseVoice 六轮加载/卸载后宿主仍上升，释放后约 1508 MiB（基线 215）；Kokoro 释放后约 947 MiB（基线 190）。逻辑实例、引用和线程结束不等于物理内存回收，F5 的 95% 回落要求不通过。
2. **取消时延不达标。** SenseVoice 约 56 秒音频取消在安静机器上 883 ms，有全量测试竞争时 1366 ms；Kokoro 长句取消 1570 ms。不能承诺所有输入 1 秒内停算。嵌入接口取消 0.49 ms 仅证明调用方返回，不证明原生计算已停止；识字原生停止确认仍缺证据。F8 整体未通过。
3. **CPU 识字资源超标。** 原生日志显示视觉计算缓冲约 4623 MiB；独立进程树峰值 9275 MiB，扫描页完成时原生进程 8750 MiB，超出原定 4 GiB 预算。单张扫描页 110.5 秒，不能宣称下限机 15 秒目标可达。输出五项文字均正确，但功能成功不等于性能通过。证据：`results/ocr-cpu-1788363559575/report.json`。
4. **语音质量与输入覆盖有限。** 没有大规模正确率评测，没有真实麦克风、格式转换、分段检测和 Bridge 平台端到端验证；完整 WAV 输出不等于生成过程实时播放。

## 本轮修复与安全边界

- 加入实际原生运行包适配源码，当前仅验证 darwin-arm64，不等于已发布所有平台运行包。
- 修复启动超时计时器未清理导致宿主退出后拖延 60 秒的问题，增加真实子进程自然退出回归测试。
- 强杀语音工作线程曾触发原生异常、退出码 134。改为串行调用、协作取消并等待原生完成和线程自然退出；不通过提前返回来掩盖未停算。后续重复加载、取消与复用未再复现崩溃，但内存/时延仍不通过。
- 主应用与独立进程使用标准输入输出协议；原生 llama-server 仅绑定本机回环随机端口，使用随机鉴权令牌、固定目的地址、禁止重定向及离线参数。不把“无外网”说成“没有任何监听端口”。
- 进程级沙箱只允许本机通信，外网负对照返回 EPERM；这证明该沙箱中的实际推理不依赖外网，不等于所有生产平台均有相同操作系统强制隔离。
- 原生日志仅在启动时转发，实际识别正文和提示词不写入启动诊断日志。安全技能用于检查输入边界、随机令牌和进程退出路径。

## 下载来源、许可和摘要

所有大资源均下载到证据根目录，未写进应用安装包或源码目录。以下为本次核验的完整 SHA-256；多连接下载对分段状态、范围、长度和最终摘要都做检查，单连接失败残包未用于导入。

| 文件 | 来源版本 | SHA-256 |
|---|---|---|
| SenseVoice 压缩包 | sherpa 官方 asr-models，2024-07-17 int8 | `7d1efa2138a65b0b488df37f8b89e3d91a60676e416f515b952358d83dfd347e` |
| Kokoro 压缩包 | sherpa 官方 tts-models，multi-lang-v1_0 | `c133d26353d776da730870dac7da07dbfc9a5e3bc80cc5e8e83ab6e823be7046` |
| Qwen Q8 GGUF | Qwen 官方仓库，370f27d7550e0def9b39c1f16d3fbaa13aa67728 | `06507c7b42688469c4e7298b0a1e16deff06caf291cf0a5b278c308249c3e439` |
| GLM f16 GGUF | ggml-org，65a42de1148dbed2297e922b5dbc7d9b70c36578 | `b06675e983db9593db78603b06f097e48c0cf078b37731c0a09612f4a249cf6f` |
| GLM 视觉投影 Q8 | 同上 | `9c4b58e33e316ed142eb5dcb41abec3844d3e6e5dc361ffb782c3fa9d175141f` |
| 本机转换 GLM Q4_K_M | b10621，从上述 f16 转换 | `3261bf7e8867827cdcf00dd341703a50d1383beeec328a6a1a40fdf16d4abaa2` |
| llama macOS arm64 运行包 | 官方 b10621 | `429c8270608600188035e5e92f7d78dffb7900904fe7dd7e6a84f48068cd13cf` |

官方来源：[SenseVoice](https://k2-fsa.github.io/sherpa/onnx/sense-voice/pretrained.html)、[Kokoro](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html)、[Qwen GGUF](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF/tree/370f27d7550e0def9b39c1f16d3fbaa13aa67728)、[GLM GGUF](https://huggingface.co/ggml-org/GLM-OCR-GGUF/tree/65a42de1148dbed2297e922b5dbc7d9b70c36578)、[llama b10621](https://github.com/ggml-org/llama.cpp/releases/tag/b10621)。

sherpa 原生包来自 npm 官方 registry 的 `sherpa-onnx-darwin-arm64@1.13.7`，下载 tarball 的 SHA-512 Base64 已对照发布完整性值核验：`5NCE50hAvr3n2pdett0SgfPBJXaFZE0bqHwbHyiq+IKZ8Ids0l4M0VrG+ImGYIafCwie+oC3uAJ+pKj9xg/k+w==`。实际加载报告 sherpa 1.13.7 / ONNX Runtime 1.28.1。

许可不能套用原任务假定：SenseVoice 模型卡标记 other，实际使用 FunASR 自定义模型许可；Qwen/Kokoro 资产声明 Apache-2.0；GLM 模型卡声明 MIT，而代码仓库 LICENSE 是 Apache-2.0，后者不能冒充权重许可。本地隔离模型已取消错误的许可证文件关联。此处只记录来源声明，不构成发布许可结论，正式权重包归档与通知义务仍待闭环。[SenseVoice 模型卡](https://huggingface.co/FunAudioLLM/SenseVoiceSmall)、[FunASR 模型许可](https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE)、[GLM 模型卡](https://huggingface.co/zai-org/GLM-OCR)。

## 尚未覆盖的范围

- PaddleOCR-VL、Qwen3-ASR-1.7B、IndexTTS-2.5/CosyVoice2 及其他精度：真实运行尚未执行。不能把下载权限问题当作阻塞，当前需要适配与验证工作。
- 原任务指定的大档语音与识字组合、所有大档语音并行组合，不能由嵌入与 GLM 的调度结果代替。
- 设置页手动导入全部交互、断点下载跨应用重启、镜像和代理、运行中删除、真实显示性能与模型观测页：仍需本机桌面端到端验收。
- Windows、Intel macOS、Linux、NVIDIA、8 GiB 下限机、正式发布签名/公证/下载源：未执行。
- Phase 5 下载目录中的旧源码 ZIP 是当时快照，**没有本轮新增适配与修复**，其校验和不代表当前工作区。

## 原验收项的当前状态

| 项目 | 本阶段状态 | 不能遗漏的边界 |
|---|---|---|
| F1 空安装基线 | 未执行真实桌面测量 | 上一阶段零权重打包检查不等于内存差值 ≤60 MB |
| F2 应用内下载/恢复 | 未执行真实产品下载链 | 本轮外部下载和手动导入不替代跨应用重启恢复 |
| F3 手动导入 | 部分通过 | 四类真实源目录导入成功；设置页两种导入交互未测 |
| F4 删除 | 仅合同通过 | 未删除本轮保留证据；Windows 文件锁未测 |
| F5 冷热与回收 | **未通过** | 四类可冷热调用/重新加载；语音物理回收不合格 |
| F6 大档互斥 | 部分通过 | 嵌入与 GLM 十轮通过；原指定的大档语音组合未测 |
| F7 大小模型并行 | 部分通过 | 本机四能力请求成功，调高预算条件下采样证实并行；默认预算未测 |
| F8 取消 | **未通过** | 安全停止不再崩溃，但时延和工作内存要求未全部满足 |
| F9 知识检索 | 部分通过 | 三格式/三问/真实向量成功；另一精度切换未测 |
| F10 图片/扫描识字 | 本机样例通过 | 图片和无文字层 PDF 实际抽取成功；不等于泛化质量与性能合格 |
| F11 桌面/Bridge 语音 | 部分通过 | 模型和隔离业务服务成功，真实麦克风上屏和外部平台未测 |
| F12 朗读 | 部分通过 | 真实音频、两个音色、断网和回转通过；交互/生成期流式未测或未完成 |
| F13 可观测 | 部分通过 | 实际调用记录含模型/后端/耗时，内存缺测不记零；峰值字段语义及页面仍待修验 |
| F14 后端覆盖 | 部分通过 | CPU/Metal 实际执行通过，设置界面展示未测 |
| F15 镜像/代理 | 未执行真实产品链 | 下载脚本不是应用内镜像/代理验收 |

## 复跑方法

保留证据根目录及当前未提交工作区后，使用 Node 24 执行以下命令；只使用隔离数据，不指向日常数据目录。并发测试与性能测试应分开运行。

```sh
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs embedding metal
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs ocr auto
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs ocr cpu
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs stt cpu
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs tts cpu
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/knowledge.mjs
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/lifecycle.mjs stt
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/lifecycle.mjs tts
node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/scheduling.mjs
```

隔离断外网示例（只影响这次调用及子进程，不改变整机网络）：

```sh
/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)(allow network* (local unix-socket) (remote unix-socket))(allow network-outbound (remote ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))' node /Users/study_superior/Downloads/lingxi-local-validation.w4b2l4/verify.mjs ocr auto
```

可将最后两项参数换成相应类别和后端复测；语音额外使用了不允许 TCP 回环的更严配置。系统进程采样在沙箱下被拒绝属于已知限制，断网结果与普通环境进程树采样必须配合阅读。

## 最终门禁与调度结果

### 真实并发调度

`results/scheduling-1788363840942/report.json`：嵌入与识字并发十轮成功，交替提交次序；40 ms 采样中原生大模型进程最大数为 1，处于加载/可用/卸载状态的大模型记录最大数为 1，确实出现排队，释放后原生进程数为 0。四能力同时请求也全部完成，两项语音与大模型共同驻留被采样捕获；排队中的识字请求取消后返回失败，未进入加载采样。

这次为缩短实验明确把大模型空闲时间设为 **100 ms**，小模型预算调至 **4096 MiB**；不是默认 120 秒空闲、1500 MiB 预算的验收结果。十轮仅覆盖 Qwen 嵌入与 GLM，不代替原任务大档语音组合。40 ms 离散采样不能排除更短瞬态；先卸后载的精确时序另由进程退出合同测试保护。

### 回归门禁

最后一轮全量测试退出 0：**13037 项通过、7 跳过、0 失败**；1294 个文件中，1293 个有通过用例，1 个 Windows 手工烟测文件全部跳过。JSON 报告将全跳过文件的顶层状态也写作 passed，不能因此说它已执行。按报告开始/最后结束时间计算约 84.33 秒，非终端总耗时。完整结果在 `results/final-vitest.json`。

此前失败轮仍保留：1292 文件通过、1 失败、1 跳过；13033 项通过、4 失败、7 跳过，83.50 秒。四项为持久化源码指纹过期，按无数据格式变化的 compatible 分类重算后才通过完整复跑。

本轮最终三段 TypeScript、范围内 ESLint（0 error、7 warning）、开放边界（1 条既有受控债务、0 新增）和 `git diff --check` 均退出 0。全仓 ESLint 的既有 `injection-scan.ts` 错误仍未改；本轮未重新打包，不能将 Phase 5 安装包当作含本轮改动的交付。

持久化指纹：`sha256:bab3e4c4166c894edc8469596611720987bd5276f843044071a19f12350b81c6`；盘点 70 stores、809 sites，CLI 闭包 10686 文件。最终进程检查未发现本实验的 llama-server 或隔离运行包进程残留；进程内语音的内存回落失败只在宿主仍存活时评价，不能用退出整个测试进程来伪造卸载达标。
