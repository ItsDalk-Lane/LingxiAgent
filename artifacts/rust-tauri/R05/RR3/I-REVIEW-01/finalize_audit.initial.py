from review_support import *
import re,difflib
s=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();old=(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/original-negative-gate.sh').read_text()
registry=re.search(r'MUTATED_FILES=\(\n(.*?)\n\)',s,re.S).group(1).split()
mutations={'N01':['rust/crates/xtask/src/stage_maps/R05.json'],'N02':['docs/rust-tauri/R05/r05_stage_pins.tsv'],'N03':['docs/rust-tauri/R05/r05_stage_pins.tsv'],'N04':['rust/crates/lingxi-service/src/credentials/mod.rs','scripts/rust-tauri/r05_t08_stage_suites.sh'],'N05':[],'N06':['rust/crates/lingxi-kernel/src/lib.rs'],'N07':['rust/crates/xtask/src/stage_maps/R05.json'],'N08':['rust/crates/lingxi-service/src/lib.rs'],'N09':['rust/crates/lingxi-adapters/src/models/tool_render.rs'],'N10':['rust/crates/lingxi-adapters/src/models/openai_completions.rs'],'N11':['rust/crates/lingxi-adapters/src/models/tool_render.rs'],'N12':['rust/crates/lingxi-service/src/runs.rs'],'N13':['rust/crates/xtask/src/stage_maps/R04.json'],'N14':['rust/crates/xtask/src/stage_maps/R05.json'],'N15':['rust/crates/lingxi-adapters/src/models/credentials.rs'],'N16':['rust/crates/lingxi-kernel/src/lib.rs']}
actual=set(re.findall(r'"(?:\$COPY)?/((?:rust|docs|scripts)/[^"\n]+\.(?:rs|json|tsv|sh))"',s[s.index('# ── N01:'):]+s[s.index('run_n03() {'):s.index('write_results() {')]))
expected=set(sum(mutations.values(),[]));assert actual==expected and len(expected)==11
assert len(registry)==len(set(registry))==12 and expected<=set(registry)
assert set(registry)-expected=={'rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs'}
identities=re.findall(r'record_case "(R05-GATE-N\d+)"',s);assert sorted(identities)==[f'R05-GATE-N{i:02d}' for i in range(1,17)]
for func,end in [('run_n03() {','write_results() {'),('write_results() {','# ── controls:')]:
 assert s[s.index(func):s.index(end)].strip()==old[old.index(func):old.index(end)].strip()
assert s.endswith('reset_copy\nwrite_results\nnote "RESULT: every R05 negative case failed closed with the gap named (16/16), controls green"\n')
(EV/'independent-implementation.diff').write_text(''.join(difflib.unified_diff(old.splitlines(True),s.splitlines(True),fromfile='original-before-I',tofile='current-candidate')))
save('mutation-scope-independent.json',{'targetsByCase':mutations,'actualMutationTargets':sorted(actual),'restoreRegistry':registry,'actualTargetCount':11,'registryCount':12,'missing':[],'extraSafetyFile':list(set(registry)-expected),'identities':identities,'defaultScope':'ALL','unchangedFunctions':['run_n03','write_results'],'finalN16ResetPresent':True})
run('final-head',['git','rev-parse','HEAD'],expected=0)
b=json.loads((EV/'source-before.json').read_text());after={p:sha(ROOT/p) for p in b['files']};changes=[p for p,h in b['files'].items() if h!=after[p]];assert not changes,changes
save('source-after.json',{'UTC':utc(),'files':after,'changes':changes,'localScopeOnly':True,'mainHEADIndexBytesEqual':True,'boundary':'只证明这些实际输入不变，不宣称H或主工作树完整冻结'})
runner=EV/'permanent-final/runner-copy'
source={str(p.relative_to(runner)):{'sha256':sha(p),'bytes':p.stat().st_size} for sub in ['rust','scripts','docs'] for p in (runner/sub).rglob('*') if p.is_file() and '__pycache__' not in str(p)}
for relative in ['rust-toolchain.toml','.gitignore','.git/HEAD','.git/index']:
 source[relative]={'sha256':sha(runner/relative),'bytes':(runner/relative).stat().st_size}
save('tested-copy-source.json',{'UTC':utc(),'root':str(runner),'files':source,'authorityBoundary':'R02 authority文件仅此新copy替换RX控制map；真实业务范围未执行；来源正文如实记录'})
inputs=['AGENTS.md','docs/rust-tauri/R05/repair-current/'+x for x in []] if False else ['AGENTS.md']
inputs += ['docs/rust-tauri/R05/repair-current/'+x for x in ['RR1_MASTER_PROMPT_2026-10-04.md','RR2_MASTER_PROMPT_2026-10-06.md','RR3_BRIEF.md','RR3_REVIEW_BRIEF.md','RR3_I_BRIEF.md','RR3_I_REVIEW_BRIEF.md','RR3_ISSUE_MATRIX.json','RR3_PROGRESS.md','RR3_HANDOFF.md']]
inputs += ['artifacts/rust-tauri/R05/RR3/I-01/'+x for x in ['REPORT.md','original-negative-gate.sh','implementation.diff','manifest.json','old-red-receipt.json','sync-order-audit.json','mutation-completeness.json','selfcheck-failure-inventory.json','source-before.json','source-after.json','b-production-counts.json']]
save('read-inputs.json',{'UTC':utc(),'fullReadFiles':{p:sha(ROOT/p) for p in inputs},'additionalRead':'原R05任务书全文、10-02专项§6/7/9/附录C-D；主negative_gate/restore_selfcheck/mutate_pin/negative_selfcheck全文；xtask main/verify/Scope/runner_identity与B镜像相关完整函数；I历史8284摘要/652原log和命令结果；G漂移/console/default命令/N06原证与COPY脚本只读','scope':'不是全阶段验收读书或运行证明'})
print('registry',len(registry),'actual targets',len(actual),'source local equal',len(after),'copy files',len(source))
