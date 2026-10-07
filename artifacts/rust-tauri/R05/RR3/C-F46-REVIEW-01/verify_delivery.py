from review_capture import *

checks = {}
problems = []
for p in EV.glob('*/command.json'):
    d = json.loads(p.read_text())
    if 'outputHash' in d:
        if sha(p.parent / 'stdout.log') != d['outputHash']:
            problems.append(str(p.relative_to(EV)))
    for row in d.get('records', []):
        index = d['records'].index(row)
        if sha(p.parent / f'build-{index}.log') != row['stdoutHash']:
            problems.append(str(p.relative_to(EV)) + ':' + str(index))
checks['allCommandOutputDigestsMatch'] = not problems
dependencies = json.loads((EV / 'isolated-restored-build/command.json').read_text())['dependencyArtifacts']
checks['all24ReusedDependenciesStillMatch'] = len(dependencies) == 24 and all(sha(pathlib.Path(k)) == v for k, v in dependencies.items())
copy_manifest = json.loads((EV / 'isolated-source-manifest.json').read_text())
copy_changes = [k for k, v in copy_manifest.items() if sha(EV / 'isolated' / k) != v]
checks['allCopiedSourceBytesRestored'] = not copy_changes
final = json.loads((EV / 'FINAL_SOURCE_BINDING.json').read_text())
checks['all321ExecutionInputBytesStillMatch'] = all(sha(ROOT / k) == v for k, v in final['executionInputComparison']['actualHashes'].items())
checks['officialAndWorkerAndEquipmentStillMatch'] = all(sha(pathlib.Path(k)) == v for k, v in final['runtimeBinary'].items())
raw = json.loads((EV / 'resources-01/f27-resource-series.json').read_text())
steady = [r for r in raw['ownerResourceSeries'] if r['phase'] == 'released-steady']
checks['each15OwnerCyclesHasThree100msWindows'] = all(len(rows := [r for r in steady if r['cycle'] == cycle]) == 3 and all(b['tMs'] - a['tMs'] >= 100 for a, b in zip(rows, rows[1:])) for cycle in range(15))
report = (EV / 'REVIEW.md').read_text()
links = re.findall(r'\]\(([^)]+)\)', report)
missing = [x for x in links if not (EV / x).exists() and x != 'MANIFEST.json']
checks['allReportEvidenceLinksExist'] = not missing
checks['reportHashMatchesResult'] = sha(EV / 'REVIEW.md') == json.loads((EV / 'RESULT.json').read_text())['reviewSha256']
checks['explicitFailuresRetained'] = json.loads((EV / 'isolated-old-command/command.json').read_text())['exitCode'] == 1 and all(json.loads((EV / f'sampler-negative-isolated/{name}/command.json').read_text())['exitCode'] == 101 for name in ['fake-fd-zero', 'fake-tcp-zero'])
qa = {'utc': utc(), 'checks': checks, 'problems': problems, 'copyChanges': copy_changes, 'missingLinks': missing, 'status': 'PASS' if all(checks.values()) else 'FAIL'}
(EV / 'DELIVERY_QA.json').write_text(json.dumps(qa, ensure_ascii=False, indent=2))
assert all(checks.values()), qa
exclude = {'MANIFEST.json', 'MANIFEST_CHECK.json'}
files = {}
for directory, dirs, names in os.walk(EV):
    dirs[:] = [d for d in dirs if d not in ('dispatch', '__pycache__')]
    for name in names:
        p = pathlib.Path(directory) / name
        relative = str(p.relative_to(EV))
        if relative not in exclude:
            files[relative] = {'sha256': sha(p), 'bytes': p.stat().st_size}
manifest = {'utc': utc(), 'root': str(EV), 'files': files, 'excluded': ['MANIFEST.json（自身）', 'MANIFEST_CHECK.json（清单验证输出）', 'dispatch/**（派发者仍可能追加的外层会话流；request已另存定格副本）', '__pycache__/**（解释器派生缓存）'], 'boundary': '只读取本包自有证据；历史报告清单另见HISTORICAL_AUDIT，主树输入另见FINAL_SOURCE_BINDING'}
(EV / 'MANIFEST.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2))
bad = [k for k, v in files.items() if sha(EV / k) != v['sha256'] or (EV / k).stat().st_size != v['bytes']]
check = {'utc': utc(), 'manifestSha256': sha(EV / 'MANIFEST.json'), 'filesChecked': len(files), 'mismatches': bad, 'status': 'PASS' if not bad else 'FAIL'}
(EV / 'MANIFEST_CHECK.json').write_text(json.dumps(check, ensure_ascii=False, indent=2))
assert not bad, bad
print(json.dumps({'delivery': qa['status'], 'checks': checks, 'manifest': check}, ensure_ascii=False))
