import datetime, hashlib, json, pathlib, re, subprocess, os
ROOT = pathlib.Path(__file__).resolve().parent
COPY = pathlib.Path('/Users/study_superior/r05t08-work/negcopy.Gz0rkR')
EV = ROOT / 'binary-bound-phases'
EV.mkdir()
CARGO = '/Users/study_superior/.cargo/bin/cargo'
ENV = dict(os.environ, CARGO_TARGET_DIR='/Users/study_superior/.cache/lingxi-r05-neg-target', CARGO_NET_OFFLINE='true')
PIN = COPY / 'docs/rust-tauri/R05/r05_stage_pins.tsv'
ORIGINAL = PIN.read_bytes()
COMMAND = [CARGO, 'test', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask', '--bin', 'xtask', 'stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites', '--', '--exact']

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def inputs():
    result = {}
    for folder in ('rust', 'scripts/rust-tauri', 'docs/rust-tauri', 'contracts'):
        for path in sorted((COPY / folder).rglob('*')):
            rel = path.relative_to(COPY)
            if 'target' in rel.parts or '__pycache__' in rel.parts:
                continue
            if path.is_file():
                result[str(rel)] = digest(path)
    result['rust-toolchain.toml'] = digest(COPY / 'rust-toolchain.toml')
    return result

def run(name, command, expected):
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    log = EV / (name + '.log')
    with log.open('w') as output:
        completed = subprocess.run(command, cwd=COPY, env=ENV, stdout=output, stderr=subprocess.STDOUT, check=False)
    body = log.read_text()
    snapshot = inputs()
    (EV / (name + '-input-manifest.json')).write_text(json.dumps(snapshot, indent=2) + '\n')
    binary = re.search(r'Running unittests src/main.rs \(([^)]+)\)', body)
    record = {'name': name, 'command': command, 'exitCode': completed.returncode, 'expectedExitCode': expected,
              'startedAt': started, 'endedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'candidateHead': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=COPY, text=True).strip(),
              'isolatedCopy': str(COPY), 'inputManifestDigest': hashlib.sha256(json.dumps(snapshot, sort_keys=True).encode()).hexdigest(),
              'logSha256': digest(log), 'lockSha256': digest(COPY / 'rust/Cargo.lock'),
              'toolchain': subprocess.check_output([CARGO, '--version'], cwd=COPY, text=True).strip(),
              'config': 'N/A: xtask compile-time pin mirror, no service configuration or network',
              'fixtureSha256': digest(PIN), 'schema': 'xtask stage maps and compile-time TSV inputs are in inputManifest',
              'binaryPath': binary.group(1) if binary else None,
              'binarySha256': digest(pathlib.Path(binary.group(1))) if binary else None,
              'testSummary': [line for line in body.splitlines() if line.startswith('test result:')]}
    (EV / (name + '-receipt.json')).write_text(json.dumps(record, indent=2, ensure_ascii=False) + '\n')
    assert completed.returncode == expected, record
    return record

try:
    before = run('normal', COMMAND, 0)
    assert '1 passed; 0 failed; 0 ignored' in (EV / 'normal.log').read_text()
    injection = run('inject', ['python3', str(COPY / 'scripts/rust-tauri/r05_t08_mutate_pin.py'), str(PIN), str(EV / 'mutation.json')], 0)
    red = run('mutated', COMMAND, 101)
    red_text = (EV / 'mutated.log').read_text()
    assert 'svc:r05_t01_model_plane' in red_text and 'drifted' in red_text and '0 passed; 1 failed; 0 ignored' in red_text
finally:
    PIN.write_bytes(ORIGINAL)
restored = run('restored', COMMAND, 0)
assert '1 passed; 0 failed; 0 ignored' in (EV / 'restored.log').read_text()
assert before['inputManifestDigest'] == restored['inputManifestDigest']
before_inputs = json.loads((EV / 'normal-input-manifest.json').read_text())
red_inputs = json.loads((EV / 'mutated-input-manifest.json').read_text())
assert [key for key in before_inputs if before_inputs[key] != red_inputs[key]] == ['docs/rust-tauri/R05/r05_stage_pins.tsv']
(EV / 'result.json').write_text(json.dumps({'findingId': 'F45', 'normalExit': 0, 'mutationExit': 0, 'targetRedExit': 101, 'restoredExit': 0, 'onlyChangedInput': 'docs/rust-tauri/R05/r05_stage_pins.tsv', 'restoredInputDigestEqual': True}, indent=2) + '\n')
print('PASS: normal 1/1 → one pin mutation → target red 101 → restored 1/1; source inputs equal and per-leg binary hashes saved')
