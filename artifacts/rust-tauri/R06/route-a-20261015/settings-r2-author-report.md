# Settings R2 最终净增量（待主控独立验收/绑定）

修复真实双服务反例：A 设置完整载入后切 B，在 B identity 尚等待时，旧 A 名称原先可回车真实 PUT B。本次以实际 resolveServerConnection 完整对象和单调 revision 标记资料归属，连接改变同步清除旧可编辑资料，render 和捕获事件两层拒绝外来资料；异步读取只有原归属仍有效才发布。同一连接等价 clone 与同 owner 缓存重开保留。助手列表和所选配置分别绑定 owner，允许真实同连接卡片管理而不渲染尚未读回的配置；已有确认框保留原对象用于明确拒绝，并未把旧对象当新连接目标。

净 patch: settings-r2-incremental.patch，14 文件，SHA256 691830d00a59f36b5b13119557da1d08638d01beed39734fc102f96a185a165a。
显式14文件窗口 base bc98c3ea5fc6b072e2ac462ee07f833053362e3a0a45d872812d066347ef4cbc → candidate 6ac8f15497576bb565564ae7d0aa1ce37479422dc655b1f6252b74c88778bc9b，详 settings-r2-source-sha256.json。这不是正式全 R06 sourceDigest，主控仍须对实际整合候选重新绑定。
前置为主控已应用 R1 42 文件、旧删除 UI 三补丁、删除 R4、Ticker R2、Mood/归档、最新 discovery_tools/discovery_windows/lib 注册。ROOT_BASELINE_SHA256.json 与 EXTERNAL_ROOT_PREREQUISITES_SHA256.json 单独记录；净增量不重复这些生产 Rust 前置，不碰 Main/登记/门禁/白名单/原叶断言，不提交。

## 原矩阵五组实证

- group0：真实供应商 summary GET 被暂停时 settings 已 ready；恢复转发前损坏真实目录资料得到真实500，错误显式而非空成功。实际 SettingsApp 卸载再挂载，同 owner 缓存立即可见无遮罩。清客户端缓存后冷初始化保留加载遮罩，真实等待至少14.9秒，15秒 forcedReady 仅移除遮罩，不把旧资料冒充已载入。随后释放实际 identity GET 才发布新 owner。该专项 fixture 用 public React mount 重开生产 SettingsApp；其余工作流使用未替换的生产 settings 入口。
- group2/共享P1：原独立 orchestrator/main/preload 脚本和原断言未改；修后两个真实 CLI、当前 Electron settings.html，B identity 延迟时旧输入不可见，B 零 PUT，A/B 真实 config 各保留原名称。原失败 original-result.json 与原脚本备份保留；修后 original-owner-frozen-result.json。全对象 identity 和 revision 也拒绝 A→B→A；既有卡片/收据与21删除子例保留。
- group14：新增 tenet 的 POST200 已真实写入文件，随后仅使实际 GET 读取损坏资料失败；页面保留旧列表并显示错误。恢复真实已写资料后 GET 找到新增原则，未谎报回滚。见 remaining-boundaries-electron.json。
- group17：带旧 health 状态撤销 memory 前置，旧状态同步清除且零 health GET；重新启用并让真实 config 读失败，显示 unavailable。再次撤销前置清除错误、零 GET，恢复启用读回真实 degraded 状态。未实现 daily pipeline 仍明确 degraded。
- group18：原生 Chromium touchCancel 产生 isTrusted=true 的 pointercancel，零 order PUT，真实顺序不变。实际 preferences.json uchg 后鼠标拖拽得到真实 order PUT500，再 GET 恢复客户端和磁盘原顺序。没有假 PointerEvent 或伪造成功响应。

## 自动/手动计费因果

Root 较新 save_config epochguard 会在用户开启自动整理后真实唤醒。旧候选只看到手动五次；当前真实有 automatic 和 manual 两个 run。现测试先等 automatic succeeded，再显式点击手动按钮，要求另一个 POST202、另一个 runId 且 trigger=manual succeeded。不能把自动结果当手动完成。
workflow-electron.json 保存两份实际报告；dream-charge-proof.json 保存真实供应商请求和费用资料。每个 run 各且仅各 atomize/dedupe/optimize/compose/verify 五项，parentRunId、modelCallId 前缀、agent、origin、Succeeded 和每项23输入/8输出全部严校；每阶段真实供应商请求恰两次，所有费用记录与所有真实供应商请求一一对应，禁止多余辅助费用或漏计。原总数5断言替换为这组更精确的双 run 分组断言，未直接改成10放过未知重复收费。

## 最终固定源验证

- settings-r2-frozen-final.log：14 父集成案例全部通过；其中19动作原工作流45个明确检查和新五组边界都经真实 HTTP/磁盘/当前 Electron。
- original-delete-21-r2-frozen-final.log：原2父/21子全部通过，原断言不改。早期恢复回归曾发现新 gate 隐藏删除按钮和确认文案，保留失败日志，修生产逻辑而非削弱旧测试。
- settings-protection-frozen-45.json：45 单元全通过（含原35保护边界与另10既有UI案例）。旧 Rust 全不可用 fixture 按新真实入口修正，原缓存清除/错误/列表与模型清空保护保留并补 owner 为 null。旧标题静态 mock 补 subscribe API，不改其9条断言。
- typecheck-frozen-root/node/test.log：三配置全通过。fmt-frozen.log：service/adapters fmt 通过。clippy-r2-final.log：当前 Root 前置真实使用 discovery 注册后的 strict Clippy 通过；此后 Rust 源未改。
- 原双服务反例最后原脚本重跑通过，original-owner-frozen.log/result.json；git apply --check 对已保存精确前置通过。

206 五个原 PARTIAL 组现已补实现和真实证据；3516 保留原删除三组、pending/重试及21保护、修共享P1入口，仍待主控独立验收登记。0B 混合写失败 UI/真实读回保留，自动开关与手动运行计费按当前 ticker 组合验证；ticker 自身停止/恢复由其既有原 suite/主控同候选流水负责，不能用本 UI 用例替代。Mac Electron+Rust 是实际执行范围；Windows/Linux/Tauri 原生资源窗口/正式打包/真实付费供应商未测。所有供应商为真实 HTTP 合成替身，核心配置、存储、Service 和UI无业务 mock。
