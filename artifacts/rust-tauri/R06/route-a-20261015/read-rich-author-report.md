# 93D5237ADE81 read 冻结交付

当前私树真实 Rust Run 范围通过，待 Root 合入当前 Main 后独立复核。只交 read 此原叶；没有扩大到 EX07/知识处理或 file/materialize。

## 原条款与实现

原三断言及 Then 原文保存在 original-assertions.json，未改原断言/门禁/登记。普通文本仍沿原分页、截断和去重行为；只读打开改用不阻塞特殊文件的句柄，20MiB输入上限，读取后核对原句柄身份、父目录身份、当前目录项和实际授权，拒绝改换来源。返回来源路径、真实SHA256和字节数；未知二进制仍明确说明未解码。

Office docx 复用已存在的有限ZIP/XML解析；xlsx 新增 xlsx_for_read -> Result<(String,usize),String>，前八表真实文字带表名，第2返回值是遗漏表数。超出八表明确文本标记、details.omittedSheets及truncated=true；空表跳过，缺少工作表、外链关系或解析失败明确错误。所有表使用同一80MiB解压预算。原 xlsx 首表预览 API 语义保持。

真实PNG/JPEG/GIF/WebP签名识别后，冻结像素只在宿主内传给现有 AuxiliaryExecutor vision 槽；共用真实路由、认证、网络和模型并发限制。未配或供应商失败是失败工具结果，不把图像base64假塞文字。真正分析文字和原始文件来源进入下一真实模型请求。成功、失败与取消均走现有费用账本，关联真实父工具调用；取消时实际请求数未知就保存NULL，不猜零次。费用尾写持有同一会话操作权，结束后同SID可以再次真实read。

## 实际证据

- final-four.log：新4父全通过，0.89秒。真实provider HTTP替身为唯一替代边界，服务/数据库/模型路由/工具 describe→mcp_call/下一模型均为生产链。
- docx、xlsx、UTF8正文进入下一真实模型；父逐文件核对原始SHA256，并核对视觉HTTP中的完整data URL与受权原始PNG字节完全一致。ImageIO/sips实际识别为2×2像素，pixel-decode.log保留。
- 坏Office、空workbook、伪PNG、不存在、20MiB超限、越权原路径与越权链接均明确拒绝；FIFO实际在3秒界内拒绝，零外部像素披露、零视觉物理请求或零收费物理请求，所有session_files为0。未配置vision有真实pre-send失败账本行，attempts=0，输入输出token未知；不是付费调用成功。
- 供应商真实400保留attempts=1/usage未知。真实取消期间供应商未返回，账本cancelled/attempts=NULL；同一cancel会话下一真实read成功，证明操作权和模型许可释放。真实重启后历史描述/账本不重复收费。
- real-ledger-facts.json是最终4父实际合成SQLite的只读查询结果，含精确home/DB/行身份，不包含认证资料。
- final-combined.log：原文件工具11、原Office非窗口5和新4通过，原断言未改。original-third-clause.log：原read失败说明父1通过。原Office窗口父本轮未跑，不借此称全部Office UI通过。
- final-four-strict.log：service lib与新测试严格检查通过。final-five-fmt.log：5源格式检查通过。无Cargo/lock新边。

## 负控与诊断保留

empty-workbook-negative-r2.log 是真实新增缺少sheets输入导致空成功的FAIL；修后同一断言通过，不减断言。empty-workbook-negative.log首轮夹具替换未生效，因此没有触及新输入，不能当反例通过或产品失败。real-first.log 的xlsx失败是合成关系文件漏Type字段，补成真实标准关系后原预期通过；不是放宽安全检查。initial-check/strict-first/final-real-source-digest等编译诊断均保留，分别为Read/Write by_ref歧义、夹具多余借用、sha2摘要格式接口，未包装为业务失败。

## 冻结与限制

source-sha.json逐5文件记录Main基线、候选和实际私树编译SHA。filetools/office/read_vision/test候选与实际编译逐字一致；lib仅四处精确接线hunk，候选保留Main额外已有前置，因此整文件SHA与私树不同。净补丁已在只复制这5源的仓库外候选上真实git apply --check及apply；Root仍须对Main最终组合编译和动态独立复核。没有改Main、登记、门禁或提交推送。

未验证Windows/Linux动态、Tauri/Electron读取窗口、真实外部付费模型、无需辅助槽的direct-image工具返回或GBK/传统二进制XLS。图像桥必须真实配置；不宣称上述能力已通过。不授予R06_ACCEPTED/R07_READY。EX07下一任务另包处理，不混本冻结源。
