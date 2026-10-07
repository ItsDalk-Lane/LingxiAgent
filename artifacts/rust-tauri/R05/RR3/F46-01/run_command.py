import datetime, hashlib, json, os, pathlib, re, subprocess, sys
root = pathlib.Path.cwd()
ev = root / "artifacts/rust-tauri/R05/RR3/F46-01" / sys.argv[1]
ev.mkdir()
command = sys.argv[2:]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
inputs = ["rust/crates/lingxi-service/src/logging.rs", "rust/crates/lingxi-service/tests/r05_t08_resources.rs", "rust/crates/lingxi-service/tests/support/r05_resource_sampler.rs", "rust/Cargo.lock", "rust-toolchain.toml"]
manifest = {p: hashlib.sha256((root/p).read_bytes()).hexdigest() for p in inputs}
with (ev/"stdout.log").open("wb") as log:
    process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, cwd=root)
end = datetime.datetime.now(datetime.timezone.utc).isoformat()
raw = (ev/"stdout.log").read_text(errors="replace")
paths = re.findall(r"^F27 raw resource series: (.+)$",raw,re.M)
if paths and pathlib.Path(paths[-1]).is_file():
    (ev/"f27-resource-series.json").write_bytes(pathlib.Path(paths[-1]).read_bytes())
record = {"command": command, "cwd": str(root), "startedAt":start, "endedAt":end, "exitCode":process.returncode, "binaryHashes": {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in [root/"rust/target/debug/lingxi-service",root/"rust/target/debug/fixture-worker"] if p.is_file()}, "head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(), "inputHashesBefore":manifest, "inputHashesAfter":{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in inputs}, "summaries": re.findall(r"test result:.*",raw), "artifacts":{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in ev.iterdir() if p.is_file()}}
(ev/"command.json").write_text(json.dumps(record,ensure_ascii=False,indent=2)+"\n")
print(json.dumps({"directory":str(ev),"exitCode":process.returncode,"summaries":record["summaries"]},ensure_ascii=False))
sys.exit(process.returncode)
