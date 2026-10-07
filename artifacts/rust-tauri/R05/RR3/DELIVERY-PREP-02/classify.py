#!/usr/bin/env python3
"""只审旧清点中的路径；不执行证据驱动，不访问仓库外原件，只写本目录。"""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent
RR = 'artifacts/rust-tauri/R05/RR3/'
PREV = RR + 'DELIVERY-PREP-01/'
READS = {}
COMMANDS = []
START = datetime.datetime.now(datetime.timezone.utc).isoformat()

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def digest(data):
    return hashlib.sha256(data).hexdigest()

def read(path):
    p = ROOT / path
    assert p.is_relative_to(ROOT) and not p.is_symlink(), path
    data = p.read_bytes()
    READS[path] = {'path': path, 'bytes': len(data), 'sha256': digest(data), 'mtimeNs': p.stat().st_mtime_ns}
    return data

def load(path):
    return json.loads(read(path))

def dump(name, obj):
    (OUT / name).write_text(json.dumps(obj, ensure_ascii=False, indent=2) + '\n')

def command(argv, label):
    start = utc()
    p = subprocess.run(argv, cwd=ROOT, capture_output=True, env={**os.environ, 'GIT_OPTIONAL_LOCKS': '0'})
    for suffix, data in [('stdout', p.stdout), ('stderr', p.stderr)]:
        (OUT / (label + '.' + suffix)).write_bytes(data)
    COMMANDS.append({'argv': argv, 'cwd': str(ROOT), 'startUTC': start, 'endUTC': utc(), 'exitCode': p.returncode,
                     'stdout': label + '.stdout', 'stdoutSHA256': digest(p.stdout),
                     'stderr': label + '.stderr', 'stderrSHA256': digest(p.stderr)})
    assert p.returncode == 0
    return p.stdout

head = command(['git', 'rev-parse', 'HEAD'], 'head').decode().strip()
branch = command(['git', 'branch', '--show-current'], 'branch').decode().strip()
staged_before = command(['git', 'diff', '--cached', '--name-only', '-z'], 'staged-before')
original = load(PREV + 'paths-resolved.json')
oldrefs = load(PREV + 'references-resolved.json')
oldlocal = load(PREV + 'local-originals.json')
by_path = {r['path']: r for r in original}
refs = collections.defaultdict(list)
for x in oldrefs:
    refs[x['target']].append({k: x.get(k) for k in ['source', 'locator', 'claimedSHA256', 'status']})

RULES = {}
def rule(key, decision, reason, driver, evidence, role):
    RULES[key] = {'id': key, 'decision': decision, 'reason': reason, 'driverReferences': driver,
                  'evidenceReferences': evidence, 'evidenceRole': role,
                  'sourceOfTruth': 'driver用途与读写位置决定；扩展名和manifest引用本身均不决定纳入'}

rule('A02_READONLY_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     '仅选择三个正常脚本作为输入留样；extra_controls导入/读取它们，实际变异在extra/*/repo和source-copy；旧校验变异另在old-validator-snapshot。不是完整实验工作树。',
     [RR+'A-02/extra_controls.py', RR+'A-02/run_checks.py'],
     [RR+'A-02/input-manifest.json', RR+'A-02/commands.json', RR+'A-02/REPORT.md'],
     'A第二轮正常源输入原文；按证据文本阅读，不在交付路径运行')
rule('A_REVIEW1_WORKCOPY', 'LOCAL_ONLY',
     'review_driver把rust/scripts/docs整体复制到snapshot并以它为运行cwd；target_controls直接变异其中helper和candidate后恢复。恢复后的hash相等仍不改变故障实验工作副本归属；复制的docs也不单独冒充本轮独立报告。',
     [RR+'A-REVIEW-01/review_driver.py', RR+'A-REVIEW-01/target_controls.py'],
     [RR+'A-REVIEW-01/input-manifest.json', RR+'A-REVIEW-01/helper-mutation.json', RR+'A-REVIEW-01/scope-mutation.json', RR+'A-REVIEW-01/delivery-inputs.json'],
     '故障实验工作副本；本地原件保留，独立驱动/原日志/变异与恢复回执继续交付')
rule('A_REVIEW2_WORKCOPY', 'LOCAL_ONLY',
     'prepare复制四个输入到snapshot，old_new直接替换其中生产shell两函数，在该目录执行旧红与恢复绿；它是受变异的可运行小副本，不是未参与变异的独立原文留样。',
     [RR+'A-REVIEW-02/review_driver.py'],
     [RR+'A-REVIEW-02/input-manifest.json', RR+'A-REVIEW-02/validator-mutation.json', RR+'A-REVIEW-02/REVIEW.md'],
     '故障校验工作副本；保存身份和复现来源，原件不远端交付')
rule('NEGATIVE_PRISTINE_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     '生产negative_gate在任何case变异前，从独立COPY逐项cp到EV/pristine；变异写COPY，reset_copy读pristine。这里是独立恢复输入，不是运行的故障COPY。保留旧恢复缺陷与历史FAIL，不把旧输入称为当前候选。',
     [RR+'I-01/original-negative-gate.sh', RR+'B-01/phase-driver.py', RR+'B-REVIEW-01/review-driver.py', RR+'G-REVIEW-01/run-default.py'],
     [RR+'B-01/REPORT.md', RR+'B-REVIEW-01/REVIEW.md', RR+'G-REVIEW-01/source-before.json'],
     '负测前独立原始源码/配置恢复基线')
rule('RESTORE_PRISTINE_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     '永久恢复回归先将主源写入copy，加入明确合法候选脏字节，再snapshot到pristine；故障写copy，恢复读取pristine。pristine曾在缺失负控中移走再按已保存字节放回，最终与同轮preservedBaseline核对；合法候选注释不属于故障注入。',
     ['scripts/rust-tauri/r05_t08_restore_selfcheck.py'],
     [RR+'I-01/REPORT.md', RR+'I-REVIEW-01/REVIEW.md', RR+'J-01/REPORT.md'],
     '同轮恢复基线文本；保留合法未提交候选而非覆盖为HEAD')
rule('RUNNER_PRISTINE_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     '恢复驱动另建runner-copy并在外侧runner-pristine留恢复输入；实际同步/中途故障发生于runner-copy。被审的8个rs是读取恢复用源码，控制authority另在runner-copy写入；不将这8项混作真实R02业务结果。',
     ['scripts/rust-tauri/r05_t08_restore_selfcheck.py', RR+'I-REVIEW-01/sync_independent.py'],
     [RR+'I-01/REPORT.md', RR+'I-REVIEW-01/REVIEW.md'],
     '生产runner同步负控的独立恢复输入')
rule('SNAPSHOT_CMP_RAW_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'snapshot-cmp-failure负控只令cmp返回2，cp已将恢复后的基线写入独立目的路径；并未将故障注入该文件。以同轮pristine原文hash相等证明它是失败时保留下来的原始输入，不冒称快照检查成功。',
     ['scripts/rust-tauri/r05_t08_restore_selfcheck.py'],
     [RR+'I-01/REPORT.md', RR+'I-REVIEW-01/REVIEW.md', RR+'J-01/REPORT.md'],
     '失败快照尝试的独立原始输入，与原失败日志一起留证')
rule('I_OLD_WORKCOPY', 'LOCAL_ONLY',
     'additional_checks建立old-independent-copy，加入合法候选后执行真实N06 append，旧reset后红，再以同一pristine和新reset恢复。即使现字节恢复仍是被执行与变异的工作副本。',
     [RR+'I-REVIEW-01/additional_checks.py', RR+'I-REVIEW-01/old-independent-functions.sh'],
     [RR+'I-REVIEW-01/old-new-independent.json', RR+'I-REVIEW-01/REVIEW.md'],
     '独立旧红/新绿的故障工作副本，原件本地保留')
rule('I_OLD_PRISTINE', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'old-independent-pristine由旧snapshot函数在N06 append之前写入，旧/新reset均读取同一原件；additional_checks证明未重拍并与baseline相等。独立恢复输入不因兄弟copy经历故障就排除。',
     [RR+'I-REVIEW-01/additional_checks.py', RR+'I-REVIEW-01/old-independent-functions.sh'],
     [RR+'I-REVIEW-01/old-new-independent.json'],
     '独立旧红与新恢复对照共用的原始基线')
rule('I_MKDIR_RECOVERED_INPUT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'mkdir故障腿断言目标不存在；随后撤去替身，用实际mkdir/cp/cmp在该独立路径写入并证明等于permanent-final/copy的合法基线。保留的是恢复成功后的输入，不能当失败腿已存在文件。',
     [RR+'I-REVIEW-01/additional_checks.py'],
     [RR+'I-REVIEW-01/REVIEW.md', RR+'I-REVIEW-01/old-new-independent.json'],
     'mkdir负控恢复腿的独立输入留样')
rule('C_FROZEN_SOURCE', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'logging-frozen.rs在工作副本之外保存正常源码；mutation目标是isolated中的logging，finalize_review三方比较frozen/isolated/main并记录恢复。单个外置正常原文不等于故障工作副本。',
     [RR+'C-F46-REVIEW-01/finalize_review.py'],
     [RR+'C-F46-REVIEW-01/mutation.json', RR+'C-F46-REVIEW-01/restoration.json', RR+'C-F46-REVIEW-01/SOURCE_AUDIT.json'],
     'F46独立审查的变异前正常源码原文')
rule('F46_ORIGINAL_SOURCE', 'INCLUDE_EVIDENCE_SNAPSHOT',
     '实施报告明确先保存logging-before.rs再修生产；DIGESTS保存其原始身份。它是旧缺陷的独立原文，不是后续编译/故障运行目录。',
     [RR+'F46-01/REPORT.md'],
     [RR+'F46-01/DIGESTS.json'],
     'F46修复前独立原文与历史红证输入')
rule('E_READONLY_SOURCE_EXCERPT', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'source-read-index逐项记录25个选读文件，24个Rust源码在本次待定集合；source-input是只读选读留样，没有Cargo项目根或运行copy。审查驱动从ROOT读取源码，在另外isolated-*.json/md变异文档；逐项将留样与读取索引及当时input-before摘要核对。创建复制命令未独立保存，不据此虚称有逐项复制命令回执。',
     [RR+'E-REVIEW-02/independent-audit.py', RR+'E-REVIEW-02/run-controls.py', RR+'E-REVIEW-02/snapshot.py'],
     [RR+'E-REVIEW-02/source-read-index.json', RR+'E-REVIEW-02/input-before.json', RR+'E-REVIEW-02/worker-source.json', RR+'E-REVIEW-02/manifest.json'],
     '文档契约核对所消费的原始源码，按非执行证据文本交付')
rule('TRIAGE_CAPTURED_SOURCE', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'collect.capture逐文件读取前后hash并把字节写入input/tag；不会在捕获文件中注入或执行。source-before/external只是从当时负测copy定向摘录20个源码/权威配置，不是复制用户配置目录，也不是继续运行的整个副本；原外部位置本轮未访问。',
     [RR+'R02-TRIAGE-01/source/collect.py'],
     [RR+'R02-TRIAGE-01/loghash/source-before.json', RR+'R02-TRIAGE-01/loghash/requirements-before.json', RR+'R02-TRIAGE-01/source/head-working-copy-comparison.json', RR+'R02-TRIAGE-01/REPORT.md'],
     '现场或要求读取时刻的独立源码/构建命令/配置原文摘存')
rule('TRIAGE_GIT_SOURCE', 'INCLUDE_EVIDENCE_SNAPSHOT',
     'HEAD/旧提交源码为只读取证中的git show原文，单文件存于source供版本差异比较；不是构建或故障运行工作目录。仅保留所声明提交的历史证据用途。',
     [RR+'R02-TRIAGE-01/source/collect.py'],
     [RR+'R02-TRIAGE-01/source/head-working-copy-comparison.json', RR+'R02-TRIAGE-01/REPORT.md'],
     '历史提交原始源码的独立摘存')
rule('SYNTHETIC_RUNTIME_TICKET', 'LOCAL_ONLY',
     'A13生产检查器用mktemp新home启动服务，p1-ticket.body是服务真实签发的临时票据响应；含运行令牌。虽属合成测试而非真实用户密钥，交付仍只登记SHA/大小/来源，不公开响应原文。取证复制件继承此边界。',
     ['scripts/rust-tauri/r02_t07_redaction_scan.sh', RR+'R02-TRIAGE-01/source/collect.py'],
     [RR+'R02-TRIAGE-01/REPORT.md'],
     '临时运行令牌原件本地保留；其请求/响应头/测试日志/失败结论保留')

# 本轮实际读取并记身份的依据；不执行这些历史驱动。
for v in RULES.values():
    for p in v['driverReferences'] + v['evidenceReferences']:
        read(p)
for p in ['docs/rust-tauri/R05/repair-current/RR1_MASTER_PROMPT_2026-10-04.md',
          'docs/rust-tauri/R05/repair-current/RR3_DELIVERY_PREP_BRIEF.md',
          'docs/rust-tauri/R05/repair-current/RR3_DELIVERY_REVIEW_BRIEF.md',
          PREV+'REPORT.md', PREV+'summary-final.json', PREV+'inventory.py']:
    read(p)

def choose(p):
    rel = p.removeprefix(RR)
    if rel.startswith('A-02/snapshot/'): return 'A02_READONLY_INPUT'
    if rel.startswith('A-REVIEW-01/snapshot/'): return 'A_REVIEW1_WORKCOPY'
    if rel.startswith('A-REVIEW-02/snapshot/'): return 'A_REVIEW2_WORKCOPY'
    if rel.startswith('I-REVIEW-01/old-independent-copy/'): return 'I_OLD_WORKCOPY'
    if rel.startswith('I-REVIEW-01/old-independent-pristine/'): return 'I_OLD_PRISTINE'
    if rel.startswith('I-REVIEW-01/mkdir-fault-pristine/'): return 'I_MKDIR_RECOVERED_INPUT'
    if rel == 'C-F46-REVIEW-01/logging-frozen.rs': return 'C_FROZEN_SOURCE'
    if rel == 'F46-01/logging-before.rs': return 'F46_ORIGINAL_SOURCE'
    if rel.startswith('E-REVIEW-02/source-input/'): return 'E_READONLY_SOURCE_EXCERPT'
    if rel.startswith('R02-TRIAGE-01/input/'): return 'TRIAGE_CAPTURED_SOURCE'
    if rel.startswith('R02-TRIAGE-01/source/'): return 'TRIAGE_GIT_SOURCE'
    if '/snapshot-cmp-failure/' in rel: return 'SNAPSHOT_CMP_RAW_INPUT'
    if '/runner-pristine/' in rel: return 'RUNNER_PRISTINE_INPUT'
    if '/pristine/' in rel:
        return 'NEGATIVE_PRISTINE_INPUT' if rel.split('/')[0] in ['B-01', 'B-REVIEW-01', 'G-REVIEW-01'] else 'RESTORE_PRISTINE_INPUT'
    raise AssertionError('需要具体判断，不能默认纳入: ' + p)

input_manifests = {}
for owner in ['A-02', 'A-REVIEW-01', 'A-REVIEW-02']:
    input_manifests[owner] = {x['path']: x['sha256'] for x in load(RR+owner+'/input-manifest.json')['files']}
e_inputs = {x['path']: x['sha256'] for x in load(RR+'E-REVIEW-02/source-read-index.json')}
e_before = load(RR+'E-REVIEW-02/input-before.json')['files']
triage_records = {}
for tag in ['source-before', 'requirements-before']:
    for x in load(RR+'R02-TRIAGE-01/loghash/'+tag+'.json')['records']:
        p = Path(x['before']['path'])
        tail = str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else 'external/'+str(p).lstrip('/')
        triage_records[RR+'R02-TRIAGE-01/input/'+tag+'/'+tail] = x

def claim(source, locator, value, actual, meaning):
    return {'source': source, 'locator': locator, 'sha256': value,
            'equalsCurrentSnapshot': value == actual, 'hashMeaning': meaning}

def baseline_for(p, key, actual):
    out = []
    owner = p.split('/')[4]
    if key.startswith('A02_') or key.startswith('A_REVIEW'):
        rel = p.split('/snapshot/', 1)[1]
        h = input_manifests[owner].get(rel)
        if h: out.append(claim(RR+owner+'/input-manifest.json', 'files/path='+rel, h, actual, '快照创建前声明的输入字节'))
    elif key == 'E_READONLY_SOURCE_EXCERPT':
        rel = p.split('/source-input/', 1)[1]
        for f, values in [('source-read-index.json', e_inputs), ('input-before.json', e_before)]:
            out.append(claim(RR+owner+'/'+f, rel, values[rel], actual, '当时读取主源的字节，不与当前主源混为一谈'))
    elif key == 'TRIAGE_CAPTURED_SOURCE':
        x = triage_records[p]
        tag = p.split('/input/',1)[1].split('/')[0]
        for fld, h in [('before', x['before']['sha256']), ('snapshot_sha256', x['snapshot_sha256']), ('after', x['after']['sha256'])]:
            out.append(claim(RR+owner+'/loghash/'+tag+'.json', x['before']['path']+'/'+fld, h, actual, '捕获时原文件读取前/捕获/读取后身份；外部原件未访问'))
    elif key in ['RESTORE_PRISTINE_INPUT', 'SNAPSHOT_CMP_RAW_INPUT']:
        divider = '/snapshot-cmp-failure/' if key == 'SNAPSHOT_CMP_RAW_INPUT' else '/pristine/'
        prefix, rel = p.split(divider, 1)
        result = prefix+'/result.json'
        if (ROOT/result).exists():
            r = load(result)
            if rel in r.get('preservedBaseline', {}):
                out.append(claim(result, 'preservedBaseline/'+rel, r['preservedBaseline'][rel], actual, '该轮合法候选的恢复前基线'))
        if key == 'SNAPSHOT_CMP_RAW_INPUT':
            raw = prefix+'/pristine/'+rel
            out.append(claim(raw, 'file-content', digest(read(raw)), actual, '同轮恢复基线实物'))
    elif key == 'RUNNER_PRISTINE_INPUT':
        prefix, rel = p.split('/runner-pristine/', 1)
        raw = prefix+'/pristine/'+rel
        # runner从主源复制；kernel不含copy一侧刻意加入的合法脏注释，两者不能错误强比。
        if (ROOT/raw).exists() and not rel.endswith('lingxi-kernel/src/lib.rs'):
            out.append(claim(raw, 'file-content', digest(read(raw)), actual, '同轮普通pristine同源非kernel输入'))
    elif key == 'I_MKDIR_RECOVERED_INPUT':
        rel = p.split('/mkdir-fault-pristine/',1)[1]
        raw = RR+'I-REVIEW-01/permanent-final/pristine/'+rel
        out.append(claim(raw, 'file-content', digest(read(raw)), actual, 'mkdir恢复腿的合法候选实物'))
    elif key in ['I_OLD_PRISTINE', 'I_OLD_WORKCOPY'] and p.endswith('lingxi-kernel/src/lib.rs'):
        f = RR+'I-REVIEW-01/old-new-independent.json'
        out.append(claim(f, 'baseline', load(f)['baseline'], actual, '故障前合法候选字节，恢复后的工作副本相等不改变归属'))
    elif key == 'C_FROZEN_SOURCE':
        f=RR+'C-F46-REVIEW-01/mutation.json'
        out.append(claim(f, 'normal', load(f)['normal'], actual, '独立审查变异前正常源码'))
    elif key == 'F46_ORIGINAL_SOURCE':
        f=RR+'F46-01/DIGESTS.json'
        out.append(claim(f, 'logging-before.rs', load(f)['logging-before.rs'], actual, '保存的修复前源码'))
    elif key == 'NEGATIVE_PRISTINE_INPUT':
        rel=p.split('/pristine/',1)[1]
        if owner=='G-REVIEW-01':
            f=RR+owner+'/source-before.json'; h=load(f)['files'].get(rel)
            if h: out.append(claim(f, 'files/'+rel, h, actual, '本轮门禁开始前源输入摘要'))
        elif owner=='B-REVIEW-01':
            f=RR+owner+'/source-before-production.json'; rows=load(f)['files']
            h=next((x['sha256'] for x in rows if x['path']==rel), None)
            if h: out.append(claim(f, 'files/path='+rel, h, actual, '本轮N03开始前生产输入摘要'))
    return out

decisions=[]
for old in original:
    if old['category'] != 'REVIEW_SOURCE_SNAPSHOT': continue
    p=old['path']; key=choose(p); ruledata=RULES[key]
    data=read(p); current=READS[p]
    historical = [claim(x['source'], x['locator'], x['claimedSHA256'], current['sha256'], '历史manifest保存的证据字节') for x in refs[p] if x.get('claimedSHA256')]
    decisions.append({'path': p, 'owner': old['owner'], 'previousCategory': old['category'],
                      'decision': ruledata['decision'], 'ruleId': key, 'reason': ruledata['reason'],
                      'evidenceRole': ruledata['evidenceRole'], 'bytes': len(data), 'sha256': current['sha256'],
                      'statEqualsPrep01': current['mtimeNs']==old['mtimeNs'] and len(data)==old['bytes'],
                      'localOnly': ruledata['decision']=='LOCAL_ONLY', 'remoteOriginalAvailable': False,
                      'remoteStatement': '本轮未交付且未核查任何远端原件可达；INCLUDE只表示拟纳入',
                      'sourceReferences': ruledata['driverReferences']+ruledata['evidenceReferences'],
                      'referenceSources': old['referenceSources'], 'historicalClaims': historical,
                      'baselineClaims': baseline_for(p,key,current['sha256']),
                      'deliveryHandling': '原件本地保留，交付摘要和driver' if ruledata['decision']=='LOCAL_ONLY' else '按不可执行证据材料阅读；不作为生产入口、测试入口或当前候选源码'})
assert len(decisions)==768

# 与已审工作副本同归属的前置误纳入文件、实际签发票据做必要补正。
supplement=[]
roots = {RR+'A-REVIEW-01/snapshot/':'A_REVIEW1_WORKCOPY',
         RR+'A-REVIEW-02/snapshot/':'A_REVIEW2_WORKCOPY',
         RR+'I-REVIEW-01/old-independent-copy/':'I_OLD_WORKCOPY'}
for old in original:
    if not old['category'].startswith('INCLUDE_'): continue
    p=old['path']; key=next((v for k,v in roots.items() if p.startswith(k)),None)
    if Path(p).name=='p1-ticket.body': key='SYNTHETIC_RUNTIME_TICKET'
    if key is None: continue
    data=read(p); h=digest(data)
    if key=='SYNTHETIC_RUNTIME_TICKET':
        obj=json.loads(data)
        assert isinstance(obj,dict) and isinstance(obj.get('ticket'),str) and obj['ticket']
    supplement.append({'path':p,'owner':old['owner'],'previousCategory':old['category'],'decision':'LOCAL_ONLY',
                       'ruleId':key,'reason':RULES[key]['reason'],'bytes':len(data),'sha256':h,
                       'localOnly':True,'remoteOriginalAvailable':False,
                       'statEqualsPrep01': READS[p]['mtimeNs']==old['mtimeNs'] and len(data)==old['bytes'],
                       'sourceReferences':RULES[key]['driverReferences']+RULES[key]['evidenceReferences'],
                       'referenceSources':old['referenceSources'],
                       'historicalClaims':[claim(x['source'],x['locator'],x['claimedSHA256'],h,'历史清单的内容摘要') for x in refs[p] if x.get('claimedSHA256')],
                       'baselineClaims':baseline_for(p,key,h) if key!='SYNTHETIC_RUNTIME_TICKET' else [],
                       'sensitiveContentPrinted':False})

overrides={x['path']:x for x in decisions+supplement}
merged=[]
for row in original:
    x=dict(row); ov=overrides.get(x['path'])
    x['previousCategory']=row['category']
    if ov:
        x.update({'category':ov['decision'],'ruleId':ov['ruleId'],'reason':ov['reason'],
                  'sha256':ov['sha256'],'bytes':ov['bytes'],'localOnly':ov['localOnly'],
                  'classificationSource':'DELIVERY-PREP-02/decisions.json' if row['category']=='REVIEW_SOURCE_SNAPSHOT' else 'DELIVERY-PREP-02/supplemental-corrections.json'})
    else:
        x.update({'localOnly':row['category'].startswith('LOCAL_'), 'classificationSource':'DELIVERY-PREP-01/paths-resolved.json',
                  'freshness':'仅继承PREP01前置快照，本轮未重审内容或刷新当前版本'})
    merged.append(x)

local_index={x['path']:{**x,'identitySource':PREV+'local-originals.json','currentCheck':'下述stat校核，无重复读取大型binary/cache'} for x in oldlocal}
oldlocal_stat=[]
for x in oldlocal:
    p=ROOT/x['path']; old=by_path[x['path']]
    s=p.lstat()
    same=s.st_size==old['bytes'] and s.st_mtime_ns==old['mtimeNs']
    oldlocal_stat.append({'path':x['path'],'sameSizeAndMtimeAsPrep01':same,'sha256Source':PREV+'local-originals.json',
                          'bytes':s.st_size,'mtimeNs':s.st_mtime_ns,'contentRehashed':False})
    local_index[x['path']]['sameSizeAndMtimeAsPrep01']=same
for x in decisions+supplement:
    if not x['localOnly']:continue
    local_index[x['path']]={'path':x['path'],'bytes':x['bytes'],'sha256':x['sha256'],
                           'type':'regular','hashMeaning':'file-content','localOnly':True,'remoteOriginalAvailable':False,
                           'ruleId':x['ruleId'],'reproductionDriver':RULES[x['ruleId']]['driverReferences'],
                           'reproductionBoundary':'未重跑；driver/摘要可复现方法，不能保证重建同字节历史原件',
                           'referenceSources':x['referenceSources'],'identitySource':'本轮实际内容SHA256'}

resolved_refs=[]
for x in oldrefs:
    r=dict(x); ov=overrides.get(x['target'])
    if ov:
        r.update({'deliveryDecision':ov['decision'],'ruleId':ov['ruleId'],'localOnly':ov['localOnly'],
                  'currentSHA256':ov['sha256'],'currentBytes':ov['bytes'],'remoteOriginalAvailable':False})
    else:
        row=by_path.get(x['target'])
        if row:r.update({'deliveryDecision':row['category'],'localOnly':row['category'].startswith('LOCAL_')})
    resolved_refs.append(r)

for name,rows in [('decisions.json',decisions),('supplemental-corrections.json',supplement),('merged-paths.json',merged),
                  ('references-with-delivery-boundary.json',resolved_refs),('local-originals.json',list(local_index.values())),
                  ('prep01-local-original-stat-check.json',oldlocal_stat)]: dump(name,rows)
for kind,selector in [('include',lambda r:r['category'].startswith('INCLUDE_')),
                      ('local',lambda r:r['localOnly']),('unknown',lambda r:r['category'].startswith(('UNKNOWN','REVIEW_','REFRESH_')))]:
    names=[r['path'] for r in merged if selector(r)]
    (OUT/(kind+'-paths.txt')).write_text(''.join(p+'\n' for p in names))
dump('classification-rules.json',list(RULES.values()))
dump('read-input-identities.json',list(READS.values()))

counts=collections.Counter(r['decision'] for r in decisions)
groups=collections.Counter(r['ruleId'] for r in decisions)
categories={}
for r in merged:
    c=categories.setdefault(r['category'],{'paths':0,'bytes':0});c['paths']+=1;c['bytes']+=r['bytes']
claims=[(r['path'],x) for r in decisions+supplement for x in r['historicalClaims']+r['baselineClaims']]
mismatches=[{'path':p,**x} for p,x in claims if not x['equalsCurrentSnapshot']]
dump('claim-mismatches.json',mismatches)
staged_after=command(['git','diff','--cached','--name-only','-z'],'staged-after')
assert staged_before==staged_after
summary={'startUTC':START,'endUTC':utc(),'head':head,'branch':branch,'isFinalFreeze':False,
         'scope':'只审PREP01的768项及同工作副本/票据必要补正；总表是旧快照合成，不含后来J/G/E/FINAL新增路径',
         'reviewedPaths':len(decisions),'decisionCounts':dict(counts),'ruleCounts':dict(groups),
         'supplementalCorrections':len(supplement),'supplementalRuleCounts':dict(collections.Counter(r['ruleId'] for r in supplement)),
         'mergedPaths':len(merged),'mergedCategories':categories,'localOriginalIndexCount':len(local_index),
         'historicalAndBaselineComparisons':len(claims),'mismatchCount':len(mismatches),
         'reviewedStatChangedSincePrep01':[r['path'] for r in decisions+supplement if not r['statEqualsPrep01']],
         'inheritedLocalOriginalStatChanged':[r['path'] for r in oldlocal_stat if not r['sameSizeAndMtimeAsPrep01']],
         'newContentBytesHashed':sum(x['bytes'] for x in READS.values()),'stagedUnchanged':True,
         'stagedBeforeCount':len([p for p in staged_before.split(b'\0') if p]),
         'commandsExecuted':'只读Git查询及本分类脚本；无构建、测试、证据driver执行、网络、stage/commit/push或原件删除',
         'securityBoundary':'仅核相关driver来源与已发现临时票据，不是完整秘密安全扫描；未读取真实用户home，不公开任何令牌原文',
         'requiredRefresh':'后续J/G/E/FINAL停止写入后必须重新实际枚举、分类、检查引用和当前输入；禁止直接套用本名单stage'}
dump('summary.json',summary)
dump('commands.json',COMMANDS)
print(json.dumps({'reviewed':len(decisions),'decisions':dict(counts),'supplemental':len(supplement),
                  'groups':dict(groups),'comparisons':len(claims),'mismatches':len(mismatches),
                  'statChanged':len(summary['reviewedStatChangedSincePrep01']),'inheritedStatChanged':len(summary['inheritedLocalOriginalStatChanged'])},ensure_ascii=False))
