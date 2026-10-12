import hashlib, json, os, pathlib, signal, subprocess, sys, time, uuid
# 在真正执行前后分别读取二进制，保留原退出码，不改变测试参数。
argv = sys.argv[1:]
binary = pathlib.Path(argv[0]).resolve()
root = pathlib.Path(os.environ['LINGXI_W03_EXECUTION_BIN_DIR'])
root.mkdir(parents=True, exist_ok=True)
record_path = root / (str(uuid.uuid4()) + '.json')
def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()
r = {'argv': argv, 'cwd': str(pathlib.Path.cwd()), 'actual_binary': str(binary), 'before_execution_sha256': sha(binary), 'started': time.time(), 'actual_home': os.environ.get('LINGXI_HOME'), 'tmpdir': os.environ.get('TMPDIR'), 'runner_sha256': sha(pathlib.Path(__file__)), 'completed': False}
record_path.write_text(json.dumps(r, indent=2))
p = subprocess.Popen(argv)
def forward(signum, frame):
    if p.poll() is None:
        p.send_signal(signum)
signal.signal(signal.SIGTERM, forward)
signal.signal(signal.SIGINT, forward)
r['child_pid'] = p.pid
r['raw_exit_code'] = p.wait()
r['finished'] = time.time()
r['after_execution_sha256'] = sha(binary) if binary.is_file() else None
r['completed'] = True
r['same_binary_bytes'] = r['before_execution_sha256'] == r['after_execution_sha256']
record_path.write_text(json.dumps(r, indent=2))
sys.exit(r['raw_exit_code'] if r['raw_exit_code'] >= 0 else 128 - r['raw_exit_code'])
