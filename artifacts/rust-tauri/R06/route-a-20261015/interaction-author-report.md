交互三叶交付：94CB746D04B7 / B34F2B298BFA / EE2ACB390FCE。

基线 c095f606c603ae08862ceafe73157c18dd13c97d；源码仍在 /tmp/lingxi-skills-completion-20261015-r1。interaction-three.patch 是19文件净补丁，interaction-three-manifest.json 给出每文件基线/候选SHA256及候选内容集合SHA256。集合SHA是源码内容标识，不是Git提交。原完整断言原样另存 interaction-original-three-leaves.json。无提交、推送、门禁或计数登记，无Main修改，无新增依赖或target。

实现：
- ask_user 正式注册进生产工具网关。问题与选项在生成确认卡之前验证；确认按key映射稳定value和label，未知值丢弃，多选去重；超时只取推荐项并标timedOut/autoSelected，无推荐项保留unanswered；拒答/确认存储中止返回空answers。实际Run取消清理挂起卡，晚到回答拒绝。会话不存在、确认存储不可用、无效题目均明确失败。
- todo_write 正式执行整份清单替换，沿用todo-state-v1记录、版本、现有面板读/完成/取消/收纳。可选activeForm、blockedReason保留到SQLite与页面；旧记录缺席时旧哈希保持。状态错误、空白/重复条目、blocked缺原因在写入前拒绝，原清单不覆盖。四处旧测试构造只补None，没有改原断言。
- show_card（原hana_card对应工具名）生成真实cardId，title/code保存在会话业务记录与实际工具结果；页面重开/服务重启可恢复。复用现有PreviewService建立临时HTML文档，iframe仅allow-scripts且无same-origin许可。代码只在iframe中执行，不写源文件。权限沿当前Execute分类保留operate允许/ask确认/read_only拒绝；没有误用“operate也必须人工确认”的删除专用Review分类。确认摘要绑定title和代码摘要，拒绝不创建记录、不渲染。
- 新增4个独立React组件，ChatPage仅import+在输入区前mount一个InteractiveToolsPanel。复用冻结的session-connection-owner真实连接及会话代际，每600ms有界单请求轮询，切会话/换凭证后旧结果不能显示或提交。不改InputArea/stream store/ws/runs/ReviewPanel/连接保护模块。
- 工具业务记录与完整会话流事件在同一SQLite事务提交，并绑定真实run_id/attempt；事务内复核principal/session/当前attempt/generation/活动Run，提交后才广播。模型历史保护只接纳固定4类、当前Run绑定的业务条目；跨回合读取UI快照时另核真实源Run/源会话owner+agent/已验证谱系/真实user锚，快照不会冒充模型消息，实际工具结果仍从原档案恢复。未知custom仍拒绝。两处hunk已与聊天作者协商，保留其UserMedia扩展。

验证证据：
- interaction-completion-delivery.log：最终生产源码7/7父证明PASS。包括真实按key作答、超时/拒答/中止/不可用、4态清单/错误不覆盖、真实批准/拒绝、当前Tauri界面及沙箱、历史篡改负控、事务身份负控和真实Run取消。
- 本轮最终原生界面证据 /private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-interaction-790165cf7a935f6b/interactive-ui-evidence。ui-receipt.json有6组硬检查；question.png/todo.png/card.png，native-driver/runner日志均保留。临时host直接import生产组件，HTTP连接当前Rust；不是mock网络或只检查iframe属性。真实iframe脚本自行尝试parent.document，得到SecurityError；window.origin为null、require不存在、宿主sentinel不变；按钮处理器真实执行并显示“卡片点击 1”，可收起/重开；审查拒绝零iframe、换凭证清旧UI。服务实际重启后读回同cardId/title/code及完整清单。已目视核对前轮三张与最终卡片图。
- 归属负控：固定4类型×缺Run/异owner/缺user锚共12个真实SQLite分支；历史明确拒绝，实际HTTP执行也拒绝，Provider调用计数不增加。事务端旧attempt、错generation、异principal、错session、terminal Run均零业务消息、零业务事件。真实Run取消后无answers记录且晚到回答409。
- interaction-todo-history-regression-final.log：原history7+todo5全PASS；interaction-original-regression-final.log中原持久化5全PASS，以及会话5通过。原断言均保留。
- interaction-ui-typecheck-final.log：生产组件定向TypeScript PASS；interaction-native-clippy-final.log：原生父测试严格Clippy PASS；interaction-service-clippy-delivery.log：最终service lib+父测试严格Clippy结果。
- interaction-baseline-proof/raw-original-failure.log：在私树真实恢复c095基线后，实际HTTP界面资料入口404，期望200而退出101；result.json记精确命令与基线；finally完整恢复候选。探针源码保留在同目录，非业务补丁；interaction-baseline-run.py可复现。

额外旧会话回归明确失败，未计为PASS：production_cli_checkpoint_tools_human_review_deferred_rewind_and_frozen_prompt 在最终deferred-rewind读取遇500 file_changed_or_not_authorized。原始 interaction-session-cli-regression-final.log；对应证据 /var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06t03-svc-production-checkpoint-model-70729-1792034017847976000。Snapshot读与原sessiontools原子替换写并发，稍后文件已completed；Root已接管专用receipt_io串行修复。该文件不在本补丁，不靠放宽检查或重试碰运气变绿。更早两次失败为私树缺原测试脚本/vite.csp-profiles前置，已从c095补齐、逐项SHA记在extra/vite-prerequisite.json；它们不在业务补丁。

复现：进入私树，CARGO_TARGET_DIR=/private/tmp/lingxi-ex01-target、CARGO_INCREMENTAL=0，使用 /Users/study_superior/.cargo/bin/cargo +1.98.1 test --locked --manifest-path rust/Cargo.toml -p lingxi-service --test r06_interactive_completion -- --nocapture。原生父自动使用已保留 ~/.cache/lingxi-r06-builds/tauri-resources，动态选端口，不复制target。所有UI构建和收据写合成home的临时目录，不写仓库证据目录。

范围与限制：真实生产工具执行/权限服务/存储/事件/当前Tauri组件均运行；模型提请求的ProviderPort是确定性替身，未调用外部付费模型。真实GUI仅macOS，Windows/Linux NOT_RUN。整份ChatPage未替换；lib/auth/adapter/model_history仅相应小hunk，合并时保留Root和聊天作者当前改动。正式三叶计数由Root合并后的当前候选与门禁决定。
