# R07-T05 Office / Beautify 实施中记录

状态：**实施中，未注册组合根，未完成 Rust 编译和集成验证，不构成 T05 通过证据。**

本记录仅涵盖本分工的 14 叶；Media / Jimeng 的 7 叶由独立分工实施。不得据本记录推进阶段状态或复用 T04、R06 通过结论。

## 本分工责任叶

下表叶 ID 均以 `R00-T02-LA-` 为前缀。

| 能力 | 叶 ID | 当前源码落点 |
| --- | --- | --- |
| Office html-to-pdf | B103E4D06D3B | `office_tools.rs`、`office_pdf.rs` |
| Office list-capabilities | 5542B3D7EDBB | `office_tools.rs` |
| Office read-document | 61EEE9F4CDD9 | `office_tools.rs`、`office-document-worker.ts` |
| Beautify apply-cover-candidate | 3F4FF7BA0528 | `office_tools.rs` |
| Beautify create-cover | F5E8EAC12C42 | `office_tools.rs` |
| Beautify cover-style-guide | 4E810F7FA63E | `office_tools.rs` |
| Beautify html-style-guide | 4A43314AE253 | `office_tools.rs` |
| Beautify list-capabilities | A0C824E245A0 | `office_tools.rs` |
| 美化 status HTTP | DA37966CFEAC | `office_routes.rs` |
| 美化生成 HTTP | C69342F075A2 | `office_routes.rs`，后台 Run 接线未完成 |
| 应用封面 HTTP | A5745E27F494 | `office_routes.rs` |
| 应用图库封面 HTTP | 988A120F3B2C | `office_routes.rs` |
| Beautify 开关 | A2ED86212AC8 | 真实来源 agent 的 `tools.disabled`，网关与执行阶段复核 |
| Office 开关 | D09857E2B057 | 同上 |

## 已编写的执行边界

- 八个工具沿正式 Plugin 来源、Read / Review 权限、统一网关和真实调用记录；写入前核对当前 Run、attempt、generation、session owner 和真实来源助手。关闭功能后旧确认不能绕过开关。
- 文档读取只把已授权快照字节交给单次监督 worker。复用现役 Mammoth、ExcelJS、unpdf；不向 worker 传文档路径或资源目录许可，不提供模型回调。子进程沿现有 R04 超时、取消、终止与回收机制，不宣称存在额外操作系统沙箱。
- DOCX / XLSX / XLSM 在宿主先做有界归档检查：最多4096成员，单成员20MiB，总展开80MiB；拒绝越界路径、链接、重复成员，不展开为磁盘文件。
- DOCX 保留 text / html；XLSX / XLSM 保留 text / json 与表格限额；PDF 保留分页、续读、无 OCR 提示。未新增 PPTX 阅读或 Office 编辑。
- PDF 复用已选受控 Chromium 端口。资产逐项 ResourceIO 授权后固定为字节，只提供内部精确 URL；普通超链接保留，外网加载资产显式拒绝。三族字体来自宿主注入的可信安装资源。浏览器实现、真实渲染与质量门禁由对应分工负责。
- 两个封面工具仅应用已有图片。保留 Markdown 正文与无关 frontmatter，图片实际复制后才写入 cover；部分结果和未知结果不伪称成功。
- 四个 HTTP 保留原字段形状和必要别名。远端不能提交本机绝对路径获取本机 owner 权限；只能使用已授权 workbench target 与上传图片字节。敏感身份未知字段拒绝。
- HTTP 生图仅提供正式后台端口接口；未接实际持久 Run 前明确返回不可用，不返回伪造 running 或活动结果。

## 已执行的局部验证

这些结果只覆盖当时的 TS reader / worker，不证明尚未编译的 Rust 模块可执行。

| 检查 | 实际结果 |
| --- | --- |
| `npx tsc --noEmit -p tsconfig.node.json` | 退出0；此前 ExcelJS bytes 类型错误已修复 |
| Office worker 单文件 nodenext 类型检查 | 退出0 |
| `npx tsc --noEmit -p tsconfig.test.json` | 退出0，包含新增真实 worker 测试 |
| `npx vitest run tests/office-plugin-tools.test.ts --maxWorkers=1` | 原有15项通过 |
| `npx vitest run tests/r07-office-document-worker.test.ts tests/office-plugin-tools.test.ts --maxWorkers=1` | 两文件24项通过，含新增9项真实子进程契约；日志 `/private/tmp/lingxi-r07-t05-office-worker-r1.log` |
| 正式 worker 接真实 DOCX / XLSX / 文本 PDF 字节 | 三种格式成功，内容包含样本真实文字或单元格 |
| 正式 worker 接扫描 PDF 字节 | 明确返回无可提取文字及 OCR 不支持 |
| 实际 esbuild 产物运行 | 生成 mjs 后接真实 DOCX 成功；Node 使用384MiB V8堆预算与清理后的环境 |
| Worker 大帧边界 | 实际50MiB文本字节成功且输出受截断预算限制；50MiB+1字节拒绝；超过72MiB整帧退出2、不输出假结果；输入扩大不改变16MiB输出上限 |
| 独立 Rust 文件 rustfmt | 已通过，仅证明格式和语法可解析 |

## 未闭项

1. 组合根注册 `OfficeApi`、工具、HTTP、可信资源目录和安装 worker argv；需要首次真实 Rust 编译。
2. 正式后台 Media Run 的受理、持久生成收据、准确目标绑定、产物应用和重启恢复；不得直接凭 agent + target 发付费请求。Media / Beautify 共用实际 job / taskId。
3. ResourceIO 无副作用写入预验：按真实 OpContext、typed target、实际挂载 Write 租约及当前版本，在媒体受理或付费前检查；最终写入再次检查，不能用原样写回探测权限。
4. 受控二进制发布：普通文本写保护仍8MiB；现有封面上传25MiB及 PDF 产物50MiB需要窄的原子、取消感知、版本校验发布方法。目前代码仍调用现有受限写路径，不能据此声称大文件兼容。
5. 受控二进制快照：当前正式 ResourceIO 文件读取上限20MiB，现有图片路径输入也受此限制。若保留25MiB封面及50MiB会话文档，需要独立受控字节读取，不扩大普通JSON读取，也不物化后绕授权直接读取。
6. PDF 实际 Chromium 渲染、失败清理与 R01 样本语义 / 字体 / 分页 / 图片视觉验收。R01 已记录 `preferCSSPageSize=false` 遇 CSS `@page size` 的引擎差异，不能把文件存在当作质量通过。
7. T04 冻结完成后才接公共 UI 路由、真实状态与重试配套；当前公共 UI 没有 T05 新路由改动。
8. Rust集成与适用门禁。嵌套 `tests/r07_office/mod.rs` 已写9项真实 Run / HTTP 测试，当前未设顶层入口、不参与 Cargo 自动集成测试，尚未执行；独立模块内4项 unit 也未执行。办公专用内部请求预算与实际 worker 均已设置72MiB，输入50MiB；原JSON/工具参数预算未扩大，但宿主20MiB读边界尚待受控快照接口，不能把 worker 单侧测试说成真实服务已支持50MiB。

注册组合根后，测试入口可采用 `#[path="r07_office/mod.rs"] mod office;`。Cargo 由主代理统一调度。
