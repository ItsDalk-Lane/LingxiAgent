#!/usr/bin/env python3
"""用隔离损坏样本实测 R08 准备地图检查器；不是正式运行验收门禁。"""
import argparse
import copy
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MAP = ROOT / 'docs/rust-tauri/R08/R08_LEAF_MAP.json'
CHECKER = ROOT / 'scripts/rust-tauri/r08_leaf_map.py'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def mutations(source):
    yield '原准备地图保持未验收状态', source, 0
    value = copy.deepcopy(source)
    value['baseScenarios'].pop()
    yield '漏基础场景必须拒绝', value, 1
    value = copy.deepcopy(source)
    leaf = value['leaves'].pop()
    del value['scenarios'][leaf['id']]
    value['count'] -= 1
    value['counts']['supplementalLeaves'] -= 1
    yield '漏补充叶并伪改数量必须拒绝', value, 1
    value = copy.deepcopy(source)
    value['leaves'][0]['assertionCases'].pop()
    yield '漏原断言必须拒绝', value, 1
    value = copy.deepcopy(source)
    value['leaves'][0]['assertionCases'][0]['originalAssertion'] = '只需成功一次'
    yield '弱化原断言必须拒绝', value, 1
    value = copy.deepcopy(source)
    # 旧测试只能作为调查候选；没有本阶段证据不能变成已完成。
    value['leaves'][0]['antecedentEvidenceIsR08Pass'] = True
    value['leaves'][0]['status'] = 'PASS'
    yield '冒用前阶段通过必须拒绝', value, 1
    value = copy.deepcopy(source)
    case = value['leaves'][0]['assertionCases'][0]
    case.update(bindingStatus='BOUND', executionStatus='PASS', implementationStatus='PASS', evidence=[])
    yield '空证据伪造通过必须拒绝', value, 1
    value = copy.deepcopy(source)
    value['additionalObligations'] = []
    yield '漏替代浏览器宿主义务必须拒绝', value, 1
    value = copy.deepcopy(source)
    value['storePlans']['additionalRustPersistencePoints'] = []
    yield '漏新增Rust存储必须拒绝', value, 1
    # 最后重新使用原输入，证明失败样本没有污染权威地图。
    yield '移除损坏输入后原地图仍有效且未验收', source, 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    source = json.loads(MAP.read_text())
    names = [MAP, CHECKER, Path(__file__).resolve(),
             ROOT / 'docs/rust-tauri/R00/ACCEPTANCE_MAP.json',
             ROOT / 'docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json']
    bindings = {str(p.relative_to(ROOT)): digest(p) for p in names}
    report = {'schema': 'lingxi.r08.map-integrity-preparation.v1',
              'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'sourceSha': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'sourceBindings': bindings, 'cases': [],
              'boundary': '实际调用准备地图检查器；不证明业务执行、正式R08门禁或双写/重复外发检查'}
    loader = '''import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("r08_map_checker", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.DEST = pathlib.Path(sys.argv[2])
sys.argv = [str(module.__file__), "--check"]
module.main()
'''
    with tempfile.TemporaryDirectory(prefix='lingxi-r08-map-integrity-') as temporary:
        for number, (name, candidate, expected) in enumerate(mutations(source), 1):
            fixture = Path(temporary) / f'{number:02}.json'
            fixture.write_text(json.dumps(candidate, ensure_ascii=False, indent=2) + '\n')
            argv = [sys.executable, '-B', '-c', loader, str(CHECKER), str(fixture)]
            process = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
            log = evidence / f'{number:02}.log'
            log.write_text(process.stdout + process.stderr)
            matched = process.returncode == expected
            if expected == 1:
                matched = matched and 'R08_LEAF_MAP_STALE' in process.stderr
            else:
                matched = matched and 'runtimeStatus=NOT_STARTED' in process.stdout
            report['cases'].append(dict(id=f'R08-PREP-MAP-{number:02}', name=name, argv=argv,
                fixtureSha256=digest(fixture), expectedExitCode=expected, actualExitCode=process.returncode,
                expectationMatched=matched, logSha256=digest(log)))
            print(f'{name}: exit={process.returncode} expected={expected} matched={matched}')
    report['sourceBindingsStable'] = all(digest(ROOT / name) == sha for name, sha in bindings.items())
    report['status'] = 'PASS_PREPARATION_CHECKER' if report['sourceBindingsStable'] and all(row['expectationMatched'] for row in report['cases']) else 'FAIL'
    report['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    (evidence / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    return 0 if report['status'] == 'PASS_PREPARATION_CHECKER' else 1


if __name__ == '__main__':
    sys.exit(main())
