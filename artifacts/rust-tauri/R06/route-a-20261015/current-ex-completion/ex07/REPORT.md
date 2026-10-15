# EX07 当前来源处理交付

结论：**HOST_SCOPE_PASS**。本机当前 Rust 服务已真实完成 PDF、DOCX、XLSX、CSV、HTML、网页快照的来源→原始快照→处理/解析事实→入库→检索→读取→引用链。Main 集成及更大范围由 Root 另验；本报告不给阶段封印或跨平台完成结论。

冻结补丁 `ex07-source-processing.patch` SHA256：`4d9c67421945446af418a342e70dc03f7b6ce69c1240542056277b5a3934018c`。15 文件逐项 base/candidate SHA 在 `source-sha.json`（文件 SHA256 `c95f390ee5cfb1bb23d24a9c06c9ab3c92faf03bad14125aa8eaade01d4afd41`）。原始最初共享基线保留在上级 `base/`，本包 `base/` 为格式化后的净差基线并包含 Root 已交的 URL 前置。不要整文件覆盖 Root 后续 EX08 的目录句柄、时钟、同主体 membership 和七天保留改动；本包 lib 仅模块登记及同 Arc provider 注入两行，main 仅解析 worker 早期分支九行。

## 改动和事实

- 原先二进制先 UTF-8 解码导致不可用的路径，改为先认真实 MIME、再运行当前服务独立解析子进程。PDF 用精确版本 pdf-extract 0.12.1 的真实字形回调产生页码、UTF-16 文本范围、字形项目范围和坐标；`boxKind=advance_cell` 明示是推进单元，不冒称印刷字形轮廓；方向未知明确标 `unknown`。无可提取文字返回 `needs_ocr`，真实入库任务明确 OCR 未就绪且零模型调用。
- Office 复用现有受限 ZIP/XML 解析，保留实际段落、工作表、行和单元格身份，输出明确 `structural`；XLSX 知识处理按实际单元格计预算，不把稀疏表展开为完整显示网格。原 `xlsx` 首表与 `xlsx_for_read` 八表显示/省略合同和预算保持。CSV 使用 csv 1.4.0，真实引号/多行/空行行号/数字及当前 TS 本地日期转换均有对照。
- HTML 用 scraper 0.27.0（关闭 CLI 默认特性）的真实 HTML5 DOM，保留结构路径、标题与字符范围；剔除隐藏脚本、样式、模板节点，不执行网页脚本。原简单标签扫描不再作为现役富文档路径。
- 新增 ProcessingArtifact 四元身份（原快照、processor ID、version、config hash）、结构级输出文本/locatorMap、警告、实际输出 SHA 和 parse 绑定。ready 产物按真实已保存文本复用，缺项或 SHA 不符明确失败。旧同形表的 hash 列只作加法迁移，已有行与原快照不改，缺 hash 不冒认已校验输出。删除源补齐处理事实 FK 与同快照目录清理。
- 网页沿 Root 同一 UrlProvider，先验笔记本归属再访问网络，保存真实 original/final URL、采集时间、Content-Type 与原始字节。普通构造器仍执行逐跳公网 DNS/IP、代理和大小/时间闸；无 provider 明确拒绝。本机真实 HTTP 替身仅由可信隐藏构造器绑定 `r06-fixture.invalid` 与精确 loopback 端口，模型请求没有开关，越域重定向照常拒绝；同 URL 在普通构造器下真实拒绝。
- 新发现并修正 refresh 归属缺口：真实负控先返回其他主体的来源名称、绝对路径及快照 SHA，现沿既有 `require_member_source` 在读文件前 404，来源事实不变。

## 真实验证

最终精确 Cargo 源组合运行 `/private/tmp/r06-ex07-delivery/frozen-exact-cargo-tests.log`：**53 PASS，0 FAIL**。其中新解析 10、新 HTTP/Run 9、原知识证明 25、Office 非 GUI 5、93D 原 4。唯一 filtered 是原 Office Electron 窗口父，本轮没有重跑该 GUI，不能计入本轮通过。此前相关旧服务 62 和 adapter 9 均 PASS，存在重叠，不相加包装总量。定向严格 Clippy（生产 lib/bin 与两新 test）及九个受影响 Rust 文件格式检查 PASS；日志为 `frozen-exact-cargo-strict.log`、`frozen-fmt.log`。

真实模型 Run 先 describe 再 call，查找并读取 DOCX/XLSX/CSV/两页 PDF，逐条引用再通过真实 HTTP 读回同来源正文。SQLite 实账 **17 chat + 4 文档 embedding**，供应商为本机真实 HTTP 替身，不冒称真实付费供应商；选择列与原快照/处理身份在 `real-model-ledger-facts.json`。三格式另一重启案例确认既有向量行/BLOB 和供应商调用数不变。

现役 TS `lib/knowledge/source-processors.ts` 原样运行并保存参考结果，SHA `c269dd2ae39be0dd6ae7f553dd75c6611c101a163acafd8f90c8220af56769bb`；`ts-reference.mjs`/`ts-reference-clean.log` 记录实际 SSR 运行。正式 OOXML 小文件覆盖两表、稀疏真实行/列、公式缓存、日期、扩展汉字、DOCX 标题；逐字段 Rust 对照通过。CSV 本地日期金样的环境为 Asia/Shanghai，不把该本机时区事实称为跨时区已验证。

实际公网 **GET https://1.1.1.1/cdn-cgi/trace** 200 text/plain，经真实服务导入、原始 SHA/正文检查通过。公网 HTML 未执行；example.com 当前系统 DNS 为 198.18 网段，原期待成功的实跑 502 原样保留，未关闭保护。真实本机 HTML HTTP→DOM→快照→嵌入→引用和私网重定向/非 HTML 零新增源的反例通过。

## 资源边界

单宿主解析租约只允许一个真实子进程；并发请求明确 busy，取消先 kill/reap 再释放。真实反例验证：sleep 超过五秒、输出超过 16 MiB、内存超过观测阈值、取消以及下一解析存活；PID 退出检查是实际系统检查。真实压缩 PDF **612033 字节→629145600 字节**，最终精确组合在约 **2978ms** 触发观测 RSS 超过 512 MiB 后回收，下一 PDF 正常。

已真实设置 CPU 五秒、禁止 core dump，墙钟五秒，输入 50 MiB、输出 16 MiB、正文 8 MiB、1000 页、PDF 20 万字形预算、Office 单 XML 20 万节点预算、5 万块；Office 另有受限解压/XML 预算。macOS RSS 每五毫秒观测，退出瞬间 RSS 暂缺可复检退出状态，连续缺失最多五十毫秒后明确失败。**这不是 macOS OS 硬内存限制，也不是文件系统沙盒**；瞬时峰值可能跨过轮询窗口。PDF 爆炸夹具的父进程生成时间不计入子进程五秒，并未假称整个测试五秒。

## 原失败与局限

`confirmation.json` 的 negativeEvidence 逐项链接不可改写的实际负控：脚本文字进入 HTML、快速进程 RSS 退出竞态、跨主体 refresh 200、TS CSV sheet 名差异、稀疏 XLSX 被显示预算误拒；同输入/强断言修后通过。若仅为夹具状态码、旧错误路径或全局计数准备问题，其前期日志保留，未包装成产品安全缺陷。

Linux 动态 NOT_RUN；Windows 解析监督明确 unsupported，未伪造 Job 实现或跨平台运行；本轮未运行 Electron/Tauri 窗口、完整打包或真实付费模型。Office 是结构级提取，不声称版面逐像素保真；扫描 PDF 不提供 OCR。旧事实不自动解释为新已验证输出，索引可重建也不删除原快照。本轮未改 Main、登记、原断言、门禁或研究能力，未提交/推送。

依赖精确新增仅 pdf-extract `=0.12.1`、csv `=1.4.0`、scraper `=0.27.0`（default-features=false）。57 新包、零旧版本升级/移除；原 flate2 新增所需解压边，其他旧记录只是同名版本消歧。Root reqwest 原声明保留。锁生成按 Root 明确授权一次收敛（含 prune 新增但不用的 CLI 特性），所有验证都 `--locked`；详见 `dependency-lock-diff.json`。

独立源 `/private/tmp/lingxi-r06-chat-wire`，自有复用 target `/private/tmp/r06-ex09-target`；Cargo `/Users/study_superior/.cargo/bin/cargo` 1.98.1，`CARGO_INCREMENTAL=0`，合成 LINGXI_HOME/TMPDIR。没有读取真实业务 HOME，也没有复制旧 target、.git 或历史 artifacts。
