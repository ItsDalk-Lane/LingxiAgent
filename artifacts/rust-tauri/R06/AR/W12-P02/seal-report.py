import copy, datetime, hashlib, json, pathlib, re

p = pathlib.Path(__file__).resolve().parent
root = p.parents[4]

def read(name):
    return json.loads((p / name).read_text())

def write(name, value):
    (p / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")

final_names = ["rust-final-build-03", "rust-final-tests-03", "ts-final-03", "typecheck-final-03", "fmt-final-03", "clippy-final-03", "open-window-final-03", "normal-final-03", "init-boundaries-final-04", "family-final-03", "rust-wal-snapshot-final-03", "diff-final-03", "registry-final-03", "xtask-final-build-01", "xtask-contracts-final-02", "xtask-boundaries-final-01"]
commands = []
for name in final_names:
    run = read(name + ".json")
    assert run["candidate"] == run["candidate_after"]
    assert run["candidate"]["aggregate"] == "c871be5b0c751ff47e01cbec8383f95627b49037f8184a3102c31a9e8f0dd989"
    assert run["candidate"]["lock_sha256"] == "d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4"
    assert run["guard_failure"] is None
    assert run["exit_code"] == (1 if name == "registry-final-03" else 0)
    commands.append(dict(name=name, status="FAIL_EXISTING_F14" if run["exit_code"] else "SCOPED_SELF_CHECK_PASS", evidence=[name + ".json", name + ".log"], argv=run["argv"], cwd=run["cwd"], start_utc=run["start_utc"], end_utc=run["end_utc"], exit_code=run["exit_code"], home=run["home"], supervised_pid=run["supervised_pid"], peak_parent_tree_rss_kib=run["peak_parent_tree_rss_kib"], guard=run["guard"]))

coverage = copy.deepcopy(read("coverage-input.json"))
log = (p / "rust-final-tests-03.log").read_text()
for row in coverage["r00_leaves"]:
    checks = [dict(name=name, observed_pass=bool(re.search(r"test (?:[\w:]*::)?" + re.escape(name) + r" \.\.\. ok", log))) for name in row["self_execution"]]
    assert all(x["observed_pass"] for x in checks), checks
    row["evidence"] = ["rust-final-tests-03.json", "rust-final-tests-03.log", "ts-final-03.json", "ts-final-03.log"]
    row["fresh_self_execution_binding"] = checks
    row["current_status"] = "RELATED_SCENARIOS_SELF_CHECK_PASS_NOT_WHOLE_LEAF_ACCEPTANCE"
for row in coverage["original_requirements"]:
    row["historical_actual_P01"] = row["actual"]
    row["evidence"] = ["rust-final-tests-03.log", "ts-final-03.log"]
    row["actual"] = "本候选固定副本重新执行原关联场景，Rust5套54注册测试、TS18套123通过；逐条原始要求和剩余义务保留，非完整T07验收。"
    if row["id"] == "T07-Step4":
        row["actual"] += " 四C及初始化锁/关闭未来WAL新增反例自检通过；R01状态/usage及错误收尾原保护回归通过。"
    if row["id"] == "A13":
        row["actual"] += " 原6网关工具真实测试包含scope、引用实际原文与权限边界。"
    if row["id"] == "A14":
        row["actual"] += " 原HTTP维度变化与显式FTS降级/rebuild恢复实跑，外部embedding响应仅loopback替身。"
for row in coverage["family_matrix"]:
    if "evidence" in row:
        row["historical_evidence_P01"] = row["evidence"]
        row["evidence"] = ["rust-final-tests-03.log", "ts-final-03.log", "family-final-03.log", "init-boundaries-final-04.log"]
    if "new_repair" in row:
        row["previous_repair_P01"] = row["new_repair"]
        row["new_repair"] = "本轮初始化持久journal写与关闭保护；前轮即时写门/状态/usage/错误收尾保留并在固定候选复跑。"
coverage["impact"]["STALE"] = ["W12-E01/R01/R02旧候选结论不适用当前hash；R02本身无最终判决", "P01/早期主仓及freeze01-03准备成绩不作为当前候选最终绿", "原2e+CANDIDATE05限定回归已重跑；主仓bc2 F08最终组合回归仍须FINAL/受影响Task执行", "后续F09-F11/F13/EX08改变同写链时对应四C、消费者、引用回归STALE须重跑"]
coverage["impact"]["R02_R05"] = "真实A13/A14消费者调用不是R02–R05正式闭包；STALER02–R05原到期剩余全部保留，整体正式前置链NOT_RUN。"

criteria = [
    dict(id="R06-AR-F12-C01", expected="合法future库、哨兵、必要WAL保留且明确拒绝", actual="Rust两生产缓存/TS三生产缓存拒绝999；TS原初始化窗口DELETE不再变WAL，主库相同；持有旧句柄后升级带未checkpoint帧且writer关闭，旧句柄close与future重开后DB/WAL字节及999/哨兵保留。", status="SCOPED_SELF_CHECK_PASS", evidence=["rust-final-tests-03.log", "rust-wal-snapshot-final-03.log", "open-window-final-03.log", "init-boundaries-final-04.log"]),
    dict(id="R06-AR-F12-C02", expected="真实不可写、占锁、可控I/O失败不删且可诊断", actual="原真实chmod/SQLite锁/子进程RLIMIT或SQLite空间限制测试保留并通过；新增真实readonly SQLite的journal_mode=WAL SQLITE_READONLY失败时当前/guard句柄关闭，另一个连接随后真实exclusive写事务可取得并回滚，原件未改。", status="SCOPED_SELF_CHECK_PASS", evidence=["rust-final-tests-03.log", "ts-final-03.log", "init-boundaries-final-04.log"]),
    dict(id="R06-AR-F12-C03", expected="只有属于本程序且可重建的损坏缓存受控恢复，备份/原文保留", actual="原拥有标识、损坏隔离、拒绝未知对象/链接/活跃者、备份一致性与恢复成本断言保留；正常Rust close清理WAL后静态损坏受控恢复真实通过，避免全禁checkpoint引入STORAGE_BUSY回归；真实reset trigger失败回滚唯一副本，TS ANN graph/retention/fallback原测试通过。", status="SCOPED_SELF_CHECK_PASS", evidence=["rust-final-tests-03.log", "normal-rust-after-02.log", "normal-rust-probe-binding-after-02.json", "ts-final-03.log", "family-final-03.log"]),
    dict(id="R06-AR-F12-C04", expected="真实第二连接/进程、旧句柄拒写、重启正常，未来版本不当损坏", actual="原进程锁/活跃writer与全部公开写门、查询隐写、status/usage及真实worker错误收尾复跑；TS在真正journal PRAGMA前尝试peer升级被SQLITE_BUSY，初始化结束后peer写入可成功；Rust真实SQLite锁协议验证COMMIT保留锁、NORMAL尚不释放、WAL后释放，生产两缓存supported再次开写/关闭及future WAL保护通过。", status="SCOPED_SELF_CHECK_PASS", evidence=["rust-final-tests-03.log", "ts-final-03.log", "init-boundaries-final-04.log", "normal-final-03.log"]),
]
boundaries = [
    dict(case="TS检查后/实际WAL前升级", status="ACTUAL_PRODUCTION_CONSTRUCTOR_REAL_TWO_CONNECTIONS_PASS", evidence="init-boundaries-final-04.log", lock_acquire="real BEGIN EXCLUSIVE transaction reads supported version; COMMIT retains exclusive DB lock", lock_handoff="locking_mode=NORMAL alone leaves held lock; actual journal_mode=WAL runs next", peer="upgrade before acquisition is refused NEWER without persistent write; upgrade at WAL call while lock held is SQLITE_BUSY", lock_release="after actual WAL change peer can write; constructor schema initializer uses immediate current-version check"),
    dict(case="TS已有WAL readonly预检后升级", status="ACTUAL_PRODUCTION_PASS", evidence="ts-final-03.log", detail="existing-WAL initialization avoids persistent journal change; current immediate schema check rejects upgraded future; readonly close guard refreshes WAL shared lock"),
    dict(case="TS真实PRAGMA失败", status="ACTUAL_SQLITE_READONLY_PASS", evidence="init-boundaries-final-04.log", detail="actual readonly connection executes same SQL; failedSql=journal_mode=WAL; no fabricated SQL return/throw; db.close then closeGuard.close; peer obtains exclusive transaction afterward; durable original equal", error="vector/fulltext STORAGE_INVALID with actual cause SQLITE_READONLY; ANN native SQLITE_READONLY; future errors all KNOWLEDGE_SCHEMA_NEWER"),
    dict(case="future WAL末个writer已退/同行及旧句柄close", status="ACTUAL_PRODUCTION_PASS", evidence=["init-boundaries-final-04.log", "rust-wal-snapshot-final-03.log"], detail="DB/WAL actual SHA and logical999/sentinel equal; SHM recorded but not treated as immutable durable payload; no checkpoint by refusing/failed handle; future raw snapshots include uncheckpointed frames"),
    dict(case="正常supported close/reopen/recovery", status="ACTUAL_PRODUCTION_PASS", evidence=["normal-rust-after-02.log", "rust-final-tests-03.log", "normal-final-03.log", "ts-final-03.log"], detail="Rust facts/index/vector protected supported Drop obtains actual exclusive transaction+current version, COMMIT retains exclusive until Connection drop, only then enables ordinary close checkpoint; busy/future failure retains no-checkpoint. TS retained readonly guard permits writes/automatic checkpoints without pinned snapshot; necessary sidecars may remain on orderly close, reopen continues normal writing."),
    dict(case="Rust原初始化漏洞运行复现", status="SOURCE_INFERRED_ONLY_NOT_RUN", evidence="../W12-R02/platform-interruption.json", detail="original Rust check→journal→transaction-check inferred from source; do not claim old Rust timing counterexample was executed. Current rawSQLite two-connection semantics and actual production stores were run; raw protocol is not direct Rust production race-hook execution."),
    dict(case="Rust生产初始化精确journal PRAGMA失败分支", status="SOURCE_REVIEWED_NOT_SEPARATELY_FORCED", evidence=["final-binding.json", "rust-final-tests-03.log"], detail="生产权限/锁/I/O失败及正常打开/关闭已实跑；精确定位在Rust内部journal PRAGMA调用的失败未单独注入，不把TS真实readonly或Rust rawSQLite协议等同于该分支实测。NO_CKPT_ON_CLOSE在持久操作前设置，失败保护由源码与相邻实测支持，留给新独立复核者。"),
    dict(case="facts共享open/Drop", status="RELATED_ORIGINAL_CONSUMERS_PASS_NOT_ALL_FACT_WRITER_CLOSED", evidence="rust-final-tests-03.log", detail="shared persistent init/close uses protected functions and actual original consumers passed; full fact writer downgrade policy, purge failures/cross-library lifecycle remain EX08/F11."),
]
attempts = []
for f in sorted(p.glob("*.json")):
    x = read(f.name)
    if isinstance(x, dict) and "argv" in x and "exit_code" in x:
        note = "historical/nonfinal evidence only" if f.stem not in final_names else "final fixed candidate evidence"
        if f.stem == "open-before-01": note = "EXPECTED_RED_ORIGINAL_BUG; real TS future DELETE→WAL/header changes, sentinel retained, no observed data loss"
        if f.stem == "rust-cold-build-01": note = "PREPARATION_FAILURE; new sha2 Array LowerHex formatting did not compile; original assertions not run; actual cold attempt retained"
        if f.stem == "normal-rust-before-01": note = "REAL_INTERMEDIATE_PRODUCT_REGRESSION; all-NO_CKPT supported close left WAL and controlled recovery refused STORAGE_BUSY; corrected by protected supported Drop"
        if f.stem == "rust-final-build-02": note = "STALE_PREPARATION_WARNING; redundant drop(()) only in dedicated new test; corrected before freeze04"
        if f.stem == "init-boundaries-final-03": note = "PREPARATION_MEASUREMENT_FAILURE; peer user_version read is correctly SQLITE_BUSY while owner exclusive; final04 reads via lock-owning connection; no product change"
        if f.stem == "registry-final-03": note = "EXISTING_F14_FAIL; MIGRATIONS9 registry7 missingv8/v9; not fixed/retired; not whole stage accepted"
        if f.stem == "xtask-contracts-final-01": note = "PASS_PREPARATION_FOR_STEADY_GATE_BINARY_BINDING; prior combined xtask/protocol build had unified serde_json features, exact package cargo run recompiled both artifacts. First gate passed; final02 retained steady binary before/after binding, no source edits."
        attempts.append(dict(name=f.stem, exit_code=x["exit_code"], start_utc=x["start_utc"], end_utc=x["end_utc"], evidence=[f.name, f.with_suffix(".log").name], interpretation=note))

results = dict(role="ONE_SHOT_ORDINARY_PRODUCT_REPAIR_EXECUTOR_NOT_INDEPENDENT_REVIEWER", status="READY_FOR_FRESH_REVIEWER", issue="R06-AR-F12", candidate="W12-CANDIDATE-05", binding="final-binding.json", canonical_root_aggregate=read("final-binding.json")["roots"][1]["root_aggregate"], whole_R06="NOT_ACCEPTED", independent_review="BLOCKED_PLATFORM_ACCESS_NOT_GRANTED", second_round_input="PARTIAL_REVIEW_INTERRUPTED_NO_FINAL_VERDICT", previous_reviews=dict(R01="FORMAL_FAIL_HISTORICAL", R02="PLATFORM_INTERRUPTED_NO_FINAL_VERDICT_NO_REPORT_RESULTS_VERDICT; not retried"), four_C=criteria, adversarial_and_family_boundaries=boundaries, commands=commands, counts=dict(rust=read("rust-counts-final.json"), typescript=dict(suites=18, tests=123, original_suites=18, original_tests=118), preservation="test-preservation.json; original names/assertions retained; original4s lifecycle wait retained; child helpers not counted twice"), original_T07_and_R00=coverage, attempts=attempts, build="First own target absent cold attempt failed new test formatting. Final03 incremental from own target; no target/source-tree/git copying, no final coldPASS claim.", guards="8GiB/600s test guard armed; parent descendant RSS/600s supervision enabled for each command; none fired; unmeasured short-run peak is null, not zero", scope=dict(real="SQLite databases/files/worker/storage/HTTP/gateway/message refs real; synthetic independent LINGXI_HOME per run", substitute="Only external embedding provider responses loopback", not_run=["independentReview while Daybreak unavailable", "Windows/Linux native checks", "actual paid provider", "real user data/migration", "physical power loss", "R02–R05 complete formal prerequisite chain", "all original R00 refusal matrices/refresh changed-unchanged", "full R06 combined final gates and mainbc2 F08 combination", "R07"], remaining=["F09", "F10", "F11", "F13", "EX08", "F14", "STALER02–R05 all original due remaining", "all90 upstream shares preserved in original_T07_and_R00"]), delivery_limit="No self-signed independent PASS/VERDICT, no Git writes by executor, no taskbook/registry/other-owned edits, independent fresh reviewer must be separately created by root only after platform access available.")
results["supplemental_closeout"] = dict(
    task="Same W12-P02 ordinary repair closeout, no new Repair/Reviewer",
    contracts=dict(status="SCOPED_SELF_CHECK_PASS", exit_code=0, generated_files=56, api_matrix_entries=626, evidence=["xtask-contracts-final-02.json", "xtask-contracts-final-02.log"]),
    boundaries=dict(status="SCOPED_SELF_CHECK_PASS", exit_code=0, actual="ownership+dependency rules+negative battery OK;736 features69 stores", evidence=["xtask-boundaries-final-01.json", "xtask-boundaries-final-01.log"]),
    inputs_before="xtask-input-before.json", final_binding="supplemental-final-binding.json",
    binaries_before="xtask-binaries-before-final-gates.json", binaries_after="xtask-binaries-after-final-gates.json",
    internal_commands="Top-level cargo absolute --locked -j8; existing unmodified contracts script internally uses rustup run pinned1.98.1 cargo run without --locked, with CARGO_BUILD_JOBS8 inherited and original isolated lock before/after equal; exact original script preserved. Boundaries internal cargo metadata unchanged.",
    failed_binding_check=dict(status="RECORDED_DIAGNOSED_NOT_PRODUCT_FAILURE", evidence=["supplemental-binding-attempt-01.json", "supplemental-binding-diagnosis-02.json"], actual="Final11/testbinary5/gatebinary2/isolatedlock unchanged; check wrongly required main old lock still equal while W06-P03 works its owned Cargo.lock; initial diagnostic gate-binary assumption corrected explicitly.", main_old_lock="bd7c0082ca055cec4b0b3cdedb78a186a39270d0cef3b83651065bd9b2e3f3c1", main_current_lock="3693acdef7ace5ff274f6650c9f2be0721f25ff2eb62503be0301f15537f9241"),
    combination="Main W06-owned aggregate lock still under work; latest combined source NOT_ACCEPTED_NOT_VALIDATED. Only base2e+CANDIDATE05 original lock is bound to scoped self checks.", independent_review="BLOCKED_PLATFORM_ACCESS_NOT_GRANTED")
write("results.json", results)

report = """缓存初始化的版本保护已补齐，本候选的限定本地自检完成，可交给新的独立复核者。状态：READY_FOR_FRESH_REVIEWER。独立复核整体仍为BLOCKED_PLATFORM_ACCESS_NOT_GRANTED；W12-R02是PARTIAL_REVIEW_INTERRUPTED_NO_FINAL_VERDICT，没有最终REPORT/results/VERDICT，本报告是普通修复执行者自检，不是独立验收判决。

原反例是三类TS缓存已在真实SQLite中拒绝较新版本之前把DELETE持久改成WAL：主库4个header字节变化，999与paid-original哨兵仍在，没有观察到资料丢失。Rust同族旧初始化时序只由源码推断，未运行旧Rust初始化窗口复现。两轮旧反例、第一次正式FAIL与所有失败尝试均保留，不覆盖、不替代。

修复让真正的持久初始化写受到版本保护。非WAL库先用真实exclusive事务检查，COMMIT保留锁，先设NORMAL后执行真实WAL切换；不能在事务内切换journal，也不能仅从配置名推断已持锁。已有WAL跳过持久journal切换，原迁移在即时事务里重查当前版本。ANN较新版本统一明确分类。TS关闭时先关可写句柄再关只读保护句柄；Rust默认关闭不checkpoint，只有supported版本在实际独占事务内重查并持锁到实际Drop时允许正常checkpoint。这样既保护future未checkpoint帧，也保留正常关闭和受控损坏恢复。Rust事实库共享这项open/Drop，原消费者回归通过；事实业务全族生命周期仍有EX08/F11剩余。

双连接证据直接观察：取得独占事务后COMMIT仍持锁、NORMAL本身不释放；在实际WAL调用前peer升版本得到SQLITE_BUSY，初始化后peer能写，supported新库/旧库再次打开能写。真正readonly SQLite在journal_mode=WAL返回SQLITE_READONLY，随后其他连接能取得exclusive事务，错误句柄及保护句柄已释放，原件字节相等。TS全部三缓存和Rust两缓存future WAL主库带未checkpoint帧，升级writer退出后旧句柄关闭和future重开拒绝仍保留DB/WAL字节、999和哨兵。Rust原初始化竞态本身未通过生产hook直接运行；当前SQLite协议和生产打开/写入/Drop均分别实测，不混为同一证据。

Rust生产内部精确journal PRAGMA失败分支未单独强制触发：权限/锁/I/O相邻失败及正常生产开关已实跑，提前禁checkpoint的失败保护有源码依据；TS真正readonly失败与Rust rawSQLite协议不能替代该分支的直接实测，留作新独立复核的明确边界。

固定副本base2e+CANDIDATE05实际检查：Rust5套54注册测试（原51及原断言名称保留，3个子进程helper不重复计数）；TS18套123（原118保留）；typecheck、fmt、相关clippy -D warnings与diff检查通过。原4秒生命周期等待不变。原真实worker、HTTP及网关工具执行，只有外部embedding响应为loopback替身。四C、全部原T07五Steps/三交付/A13/A14、24R00叶的限定份额和90上游剩余逐条在results.json列明；相关场景通过不等于完整T07、原叶或R06验收。

同一任务收尾补跑两项适用门禁，冻结产品未改变：xtask check-contracts exit0（56生成文件、626API条目无漂移），xtask check-boundaries exit0（736功能/69存储的所有权、依赖与负向battery通过）。命令与独立home/guard在xtask-contracts-final-02.json/log、xtask-boundaries-final-01.json/log；两运行文件在xtask-binaries-before-final-gates.json与after-final-gates.json逐SHA相同。最初合并构建因serde_json特性合并，与单包运行不同而正常重编译；首次contracts通过记录保留，final02用于精确运行文件前后绑定，没有产品变更。顶层cargo为绝对路径--locked -j8；原contracts脚本内部按rustup固定1.98.1执行cargo但本身未带--locked，继承CARGO_BUILD_JOBS8，副本原lock前后SHA不变，未改脚本或虚称内部参数。

F14登记检查registry-final-03仍失败：MIGRATIONS有9项、registry有7项，v8/v9缺登记。未修改登记、Cargo、migrations或门禁。F09–F11/F13/EX08/F14和STALER02–R05到期剩余、refresh全变化矩阵、完整正式前序链、目标OS/真实用户迁移/真实供应商等未验项目保留。R06整体NOT_ACCEPTED，不能以本项通过关闭其他事项。

最终11文件主仓与只读副本逐SHA相等，根控NUL算法aggregate为c86ab7fa171b5ae5b6c40a9715a3268d3ec1df1ab008b62dae7a095645d90d08。runner的JSON path/SHA算法aggregate是c871be5b0c751ff47e01cbec8383f95627b49037f8184a3102c31a9e8f0dd989，freeze-04含bytes的历史JSON算法又不同，不能混用。副本原2e锁SHA d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4未变，五Rust运行文件运行前、后、封存时SHA一致。主仓在根控授权F08交付后HEAD为bc2ce51fd005ea75b9949156974e15d2761eb734，11文件未漂移；本次固定候选结果不代表最新组合绿，最终组合回归由FINAL和受影响Task执行。

收尾最终SHA另见supplemental-final-binding.json：副本11产品/测试、原lock、原5测试运行文件及新增2门禁运行文件均一致。主仓W06-P03拥有的Cargo.lock在工作中从旧封存bd7变为3693acdef7ace5ff274f6650c9f2be0721f25ff2eb62503be0301f15537f9241；要求它仍等于旧主锁的一次核对断言失败已保留，初始“门禁binary变化”诊断假设经逐SHA排除，supplemental-binding-diagnosis-02.json明确纠正为主组合锁变化。未改主锁、未复跑稳定副本业务套件，也不把该变化当副本或产品失败；整体组合仍未验证。

首次自有target冷编译因新增测试sha2输出格式准备错误未能运行测试，证据保留；最终编译如实为自有target增量编译。中间全禁checkpoint引入正常恢复STORAGE_BUSY是真实产品回归，已修复并留下前后对照。新探针第一次锁外读取user_version失败是测量准备错误，改用持锁连接取证后通过，无产品修改。每次唯一尝试均保存argv/cwd/开始结束/退出码/真实独立home/候选前后输入/lock/toolSHA/平台/替身边界/父监督记录；8GiB/600s guard与父监督全程启用，没有触发。短命令未测峰值写null，不写0。

保护的用户设置从未读取或改写，其他任务改动保留；执行者没有提交、推送、改rootmeta、扩大权限或重试受阻Review。新独立Reviewer须在平台实际可用后由根控另行创建。本轮没有安全扫描，也没有自签独立PASS。

SQLite语义依据：[PRAGMA官方说明](https://www.sqlite.org/pragma.html) 与 [NO_CKPT_ON_CLOSE官方说明](https://www.sqlite.org/c3ref/c_dbconfig_defensive.html#sqlitedbconfignockptonclose)，并以实际双连接锁行为及字节对照验证。

可复核入口：final-binding.json绑定产品和运行文件；results.json保留全部逐项期望/实测/推断/未验及历史失败；evidence-manifest.json列每份原始证据SHA，manifest自身不自引用。
"""
(p / "REPORT.md").write_text(report)

files = []
for f in sorted(p.rglob("*")):
    if f.is_file() and f.name != "evidence-manifest.json":
        files.append(dict(path=str(f.relative_to(p)), bytes=f.stat().st_size, sha256=hashlib.sha256(f.read_bytes()).hexdigest()))
authority = []
paths = ["AGENTS.md", "artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md", "docs/rust-tauri/R06/repair-current/dispatches/R06-AR-W12_REPAIR_TEMPLATE.md", "artifacts/rust-tauri/R06/AR/W12-R01/REPORT.md", "artifacts/rust-tauri/R06/AR/W12-P01/REPORT.md", "artifacts/rust-tauri/R06/AR/W12-P01/results.json", "artifacts/rust-tauri/R06/AR/W12-R02/platform-interruption.json", "artifacts/rust-tauri/R06/AR/W12-R02/partial-evidence-manifest.json", "artifacts/rust-tauri/R06/AR/W12-R02/open-window-02.json", "artifacts/rust-tauri/R06/AR/W12-R02/open-window-02.log", "artifacts/rust-tauri/R06/AR/W12-R02/probe-open-window-v2.mjs", "artifacts/rust-tauri/R06/AR/W12-R02/normal-init-01.json", "artifacts/rust-tauri/R06/AR/W12-R02/normal-init-01.log", "artifacts/rust-tauri/R06/AR/W12-R02/probe-family.mjs", "artifacts/rust-tauri/R06/AR/SCOPE-01/REPORT.md", "artifacts/rust-tauri/R06/AR/W12-CANDIDATE-05/binding.json", "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/01_通用执行约束.md", "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/04_阶段依赖与接口交接.md", "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/05_验收与性能协议.md", "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md", "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json"]
for name in paths:
    f=root/name
    assert f.is_file(), str(f)
    authority.append(dict(path=name,sha256=hashlib.sha256(f.read_bytes()).hexdigest(),bytes=f.stat().st_size))
write("evidence-manifest.json",dict(role="EXECUTOR_EVIDENCE_MANIFEST_NOT_REVIEWER_VERDICT", time_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), status="READY_FOR_FRESH_REVIEWER", independent_review="BLOCKED_PLATFORM_ACCESS_NOT_GRANTED", second_round="PARTIAL_REVIEW_INTERRUPTED_NO_FINAL_VERDICT", files=files, authority_sources=authority, authority_hash_time="report seal; not claimed as a historical before-run snapshot", self_excluded="evidence-manifest.json to avoid recursive digest", candidate="final-binding.json", guard="No bypass; ordinary local checks only; independentReview not retried"))
print(json.dumps(dict(status=results["status"],files=len(files),authority=len(authority),r00=len(coverage["r00_leaves"]),upstream_remaining=len(coverage["upstream_r06_remaining_shares"])),ensure_ascii=False))
