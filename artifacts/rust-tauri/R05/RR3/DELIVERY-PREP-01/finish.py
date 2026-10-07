#!/usr/bin/env python3
"""补齐引用语义与交付说明；不执行清单中的历史测试命令。"""
from pathlib import Path
import collections, datetime, hashlib, json, os, subprocess
ROOT=Path.cwd(); OUT=ROOT/'artifacts/rust-tauri/R05/RR3/DELIVERY-PREP-01'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(b):return hashlib.sha256(b).hexdigest()
def read(n):return json.loads((OUT/n).read_text())
def write(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
start=utc(); rows=read('paths.json'); bypath={r['path']:r for r in rows}; refs=read('references.json')
g='artifacts/rust-tauri/R05/RR3/G-REVIEW-01/'
idx=json.loads((ROOT/'artifacts/rust-tauri/R05/RR3/G-INTERRUPTION-01/r02-and-binding-index.json').read_text())
for r in refs:
    if r['status']!='UNRESOLVED_REFERENCE_NOT_PROVEN_MISSING':continue
    if r['source'].endswith('/G-REVIEW-01/own-storage-receipt.json'):
        r['status']='HISTORICAL_DELETED_CLONE_OBJECT_RECEIPT';r['resolution']='原回执的已移除外置副本对象，不是RR3原始证据缺失';continue
    if r['source'].endswith('/G-INTERRUPTION-01/r02-and-binding-index.json') and '/dependencyEvidence/' in r['locator']:
        parts=r['locator'].split('/');entry=idx[int(parts[1])];dep=entry['dependencyEvidence'][int(parts[3])]
        target=g+entry['path']+'/'+dep['path'];p=ROOT/target
        if p.is_file():
            r['target']=target;r['status']='PRESENT_LOCAL';r['resolution']='依赖证据相对该条R02 evidence根解析';r['actualSHA256']=sha(p.read_bytes());r['claimedMatches']=r['actualSHA256']==r['claimedSHA256']
            if target in bypath:
                r['category']=bypath[target]['category']
                if r['source'] not in bypath[target]['referenceSources']:bypath[target]['referenceSources'].append(r['source'])
unresolved=[r for r in refs if r['status']=='UNRESOLVED_REFERENCE_NOT_PROVEN_MISSING']
write('references-resolved.json',refs);write('unresolved-references.json',unresolved)
local=read('local-originals.json');actual={r['path']:r['sha256'] for r in local}
comparisons=[]
for r in refs:
    if r.get('claimedSHA256') and r['target'] in actual:
        comparable=bypath[r['target']]['type']!='symlink'
        comparisons.append(dict(source=r['source'],locator=r['locator'],target=r['target'],claimedSHA256=r['claimedSHA256'],actualSHA256=actual[r['target']],comparable=comparable,matches=r['claimedSHA256']==actual[r['target']] if comparable else None,note='同为文件内容SHA' if comparable else '本次只哈希链接文本；旧清单可能跟随链接哈希外部程序，本次不读取外部程序，不判内容失配'))
write('local-original-reference-hash-check.json',comparisons)
write('paths-resolved.json',rows)
# 证明忽略规则的实际行为；不读运行凭证内容。
samples=['artifacts/rust-tauri/R05/RR3/C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib',
         'artifacts/rust-tauri/R05/RR3/H-REVIEW-02/verified-binaries/lingxi-service',
         'artifacts/rust-tauri/R05/RR3/H-REVIEW-02/commands/a13-final/stdout.log',
         'artifacts/rust-tauri/R05/RR3/R02-TRIAGE-01/input/request-correlation/home/lingxi-service/local-token.json',
         'artifacts/rust-tauri/R05/RR3/C-F46-REVIEW-01/isolated/target/probe',
         'artifacts/rust-tauri/R05/RR3/J-01/node_modules/probe']
cs=utc();p=subprocess.run(['git','check-ignore','-v','--no-index','--stdin'],input='\n'.join(samples)+'\n',text=True,capture_output=True,env={**os.environ,'GIT_OPTIONAL_LOCKS':'0'})
(OUT/'check-ignore.stdout').write_text(p.stdout);(OUT/'check-ignore.stderr').write_text(p.stderr)
write('check-ignore-command.json',dict(argv=['git','check-ignore','-v','--no-index','--stdin'],stdinPaths=samples,startUTC=cs,endUTC=utc(),exitCode=p.returncode,stdoutSHA256=sha(p.stdout.encode()),stderrSHA256=sha(p.stderr.encode())))
authorities=['.gitignore','docs/rust-tauri/R05/repair-current/RR1_MASTER_PROMPT_2026-10-04.md','docs/rust-tauri/R05/repair-current/RR3_BRIEF.md','docs/rust-tauri/R05/repair-current/RR3_HANDOFF.md','docs/rust-tauri/R05/repair-current/RR3_DELIVERY_PREP_BRIEF.md']
write('authority-hashes.json',[dict(path=x,bytes=(ROOT/x).stat().st_size,sha256=sha((ROOT/x).read_bytes())) for x in authorities])
s=read('summary.json');s['resolvedReferenceStatusCounts']=dict(collections.Counter(r['status'] for r in refs));s['unresolvedReferenceCount']=len(unresolved);s['localOriginalClaimComparisons']=len(comparisons);s['historicalClaimMismatchCount']=sum(c['matches'] is False for c in comparisons);s['incomparableLinkClaims']=sum(not c['comparable'] for c in comparisons);s['finishUTC']=utc()
write('summary-final.json',s)
owners={
 'A/F42':['candidate.rs','candidate/tests.rs','main.rs','runner_identity.rs','verify/runner_tests.rs','r02_t08_legacy_entry_regression.sh','r02_run_output_regression.py','run_output_sinks.py'],
 'C/F27/I10':['r05_t08_resources.rs','r05_resource_sampler.rs'],
 'F46/H/F47/F48':['logging.rs','lib.rs','redaction.rs','r02_t01_service_smoke.sh','r02_t01_log_level_regression.py'],
 'B→I→J/F45/F49/F50':['r05_t08_negative_gate.sh','r05_t08_mutate_pin.py','r05_t08_negative_gate_selfcheck.py','r05_t08_restore_selfcheck.py','r05_t08_node_selfcheck.py','r05_t08_prepare_node.py']}
prod=[]
for r in rows:
 if r['category']=='INCLUDE_PRODUCTION':
  r=dict(r);r['workPackageCandidates']=[k for k,v in owners.items() if any(r['path'].endswith('/'+x) for x in v)];prod.append(r)
write('production-scope.json',prod)
table='\n'.join('| '+k+' | '+str(v['paths'])+' | '+f"{v['bytes']:,}"+' |' for k,v in s['categories'].items())
large=[r for r in local if r['bytes']>100*1024*1024]
report=f'''# RR3 DELIVERY-PREP-01：精确交付范围前置清点

结论：**只读准备已完成；不是最终冻结，也没有暂存、提交、推送或远端原件可达证明。** 本轮只新增本目录。J及后续独立审查、G/E/FINAL尚不能由本快照代替；最终停写后必须刷新。R05/R06状态不由本报告改变。

## 对象与清点边界

快照 UTC {s['startUTC']}—{s['endUTC']}；HEAD `{s['head']}`，分支 `{s['branch']}`。真实 `git ls-files --others --exclude-standard -z` 加 tracked diff/cached diff 共 **{s['totalPaths']}条路径**，逻辑长度 **{s['totalBytes']:,} bytes**，其中包含52个Git只列目录的嵌套仓库条目，其目录长度不代表内部总大小。暂存条目0。只清点RR3与本轮实际改动；不读取真实用户home、外置target、密钥或测试运行凭证正文，不遍历忽略目录的内部文件。原始输出和命令退出/UTC/hash见 `commands.json`、`check-ignore-command.json`。

读取RR1 §3.3/5.4/6.2、RR3 BRIEF/HANDOFF与实际ignore。§3.3明确不能混target或故障副本，§5.4需要真实原件身份，§6.2需要交付与交接统一。因此“原件在本机保留”和“Git交付范围排除原件”可以同时成立，但必须写清。没有更改旧manifest、README、现行文档或历史引用。

## 逐路径分类

`paths-resolved.json` 为逐条路径/类型/字节/分类/原因/引用来源；`production-scope.json` 给21个生产修复及永久回归候选的包归属和真实当前SHA。42个现行文档/简报、14066个证据候选在 `include-paths.txt`；该文件是**审阅候选，不是无需审阅即可stage的命令输入**。768个源码/配置快照在 `review-paths.txt`，未证明非故障注入前不能自动纳入。排除仅代表不随本次Git交付，绝不授权删除。

| 分类 | 路径数 | 逻辑字节 |
|---|---:|---:|
{table}

保留正式报告、所有历史失败、原始日志、命令/退出/UTC回执、输入/来源摘要、测量序列、复现实验driver与变异说明。比如 `sampler-negative-*/fake-*/bin/lsof` 是负例驱动脚本，不能因位于bin目录就当实际binary删去。合成secret负例不自动判真实泄漏；运行home中的临时令牌、设备记录、数据库则单列本地运行状态，不公开原文。这里不是完整秘密审计，最终纳入前须结合产生该文件的driver核查来源；不凭关键词一刀切删除日志。

3442项构建缓存按用途排除，含`.o/.rlib/.rmeta`、incremental和指纹；61项实际Mach-O/ELF类装备独立登记，包含own-target内小程序，未把它们冒称普通缓存。1199项isolated/runner-copy/旧红副本按隔离工作树用途排除；52个Git只列目录的嵌套试验仓库不递归收进提交或变成gitlink。源码输入快照与故障工作副本分开待复核，不按“越大越该删”的规则处理。1个绝对Python符号链接仅本机有效。

## 原始引用与localOnly边界

读取897份正式报告/摘要/清单，实际读取hash在 `metadata-read-hashes.json`。`references.json` 是初步词法索引，**以 `references-resolved.json` 的语义校正为准**：E01消费清单按其声明的E01 manifest根解析；G中断依赖日志按每条R02 evidence根解析；旧共享objects清理回执不误当RR3原证缺失。最终状态计数：`{json.dumps(s['resolvedReferenceStatusCounts'],ensure_ascii=False)}`。未解析项 {len(unresolved)}，详见 `unresolved-references.json`；外置引用49条只记标识，未访问、不证明存在。记录计数含重复引用，不是唯一文件数；本索引覆盖所选正式材料，不能当任意自由文本路径的完备解析。

所读材料未发现真正的 `localOnly/local_only/local-only` 结构字段；测试名中的local_only不是交付声明。`local-originals.json` 新增独立本地边界索引，共967项（61装备、1大rlib、1符号链接、904项被引用隔离源码），记录本次真实内容SHA、类型、大小、引用归属。原件仍在本地；`remoteOriginalAvailable=false`表示**本轮未交付也未证明远端有该原件**，不是已检查所有远端存储。只哈希实际装备、大rlib及被引用的少量源码，未对数千缓存反复全字节哈希。符号链接hash是链接文本，不是外部Python原件。

对应历史SHA检查见 `local-original-reference-hash-check.json`：{len(comparisons)-s['incomparableLinkClaims']}条文件内容引用可比较、{s['historicalClaimMismatchCount']}条内容不同；另{s['incomparableLinkClaims']}条Python符号链接引用口径不具可比性，本次哈希链接文本，旧清单可能哈希其指向的外部程序，未访问外部程序所以不判失配。这里只展示当前实物与所引历史身份；最终所有者需说明引用时间与对象归属，不重写旧清单。

必须由后续E/最终交付明确区分的材料：R05_REPORT、R05_HANDOFF、R05_TEST_MAP、R05_ACCEPTANCE_LEDGER、R05_PERFORMANCE_RESULTS、R05_NEGATIVE_GATE_REPORT、R05_INDEPENDENT_REVIEW、PROGRESS_LEDGER、ORCHESTRATOR_PROGRESS与RR3 HANDOFF/PROGRESS/ISSUE_MATRIX。新Git交付README/manifest宜链接本地边界索引，逐项写originalPath、actualSHA256、bytes、producer命令/输入/锁/driver路径、localOnly=true、远端可用材料与复现限制；不要改旧manifest来抹去原件引用，也不要将“可重跑”写成“已取得同字节历史原件”。

重点交付分歧：`C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib` 为 **134457480 bytes**，超过简报明确的普通GitHub单文件限制，当前SHA `{large[0]['sha256'] if large else 'UNKNOWN'}`。其归属由C-F46/MANIFEST、isolated构建命令和E-02消费清单追踪。`H-REVIEW-02/verified-binaries/lingxi-service`（62352400 bytes）、各sampler装备、隔离源码也是当前原证引用的本地原件。若后续用户要求这些**历史原件必须远端取得**，本方案不能满足该部分；须具体解决这些项的交付渠道，不能用源码重建/摘要替代原件。当前不引入外部存储、发布或LFS安装。

## 可复审的下一步与限制

最终停写后，沿本脚本同一规则刷新实际Git候选、文件大小/类型、引用归属与装备hash，并新增最终清点快照；不能复用本清单冒签后续J/G/E/FINAL文件。逐项处置768个REVIEW源码快照与运行数据来源，将最终可交付内容形成新的精确路径列表。只暂存被审定的单个文件，避免`git add -A`、整个RR3目录或嵌套仓库；核暂存内容不含故障源码、缓存、实际装备、临时令牌和本机绝对工具链接，并验证新README/manifest在实际交付范围内的引用闭合。提交/推送由总控在已有有效授权范围执行，本代理不做Git写操作。

本轮仅执行只读Git查询、元数据/指定证据读取及报告生成；无Cargo、负测、测试、构建、网络、删除、Git写操作或系统改动。以上“完成”仅指前置清点完成，不能解释为产品验证通过或R05放行。已有源码/文档可被其他作者更新；快照不是原子冻结。
'''
(OUT/'REPORT.md').write_text(report)
write('finish-receipt.json',dict(startUTC=start,endUTC=utc(),operation='read-only metadata/reference resolution and own report generation',noTestsOrBuilds=True,noGitWrites=True,unresolvedReferenceCount=len(unresolved)))
print(json.dumps(dict(unresolved=len(unresolved),comparisonCount=len(comparisons),mismatches=s['historicalClaimMismatchCount'],referenceCounts=s['resolvedReferenceStatusCounts']),ensure_ascii=False))
