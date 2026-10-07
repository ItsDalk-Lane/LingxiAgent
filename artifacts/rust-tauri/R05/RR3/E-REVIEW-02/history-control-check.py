from pathlib import Path
import json,sys
root=Path.cwd();r=Path(__file__).resolve().parent;old=json.loads((root/'artifacts/rust-tauri/R05/RR3/E-01/before/docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json').read_text());new=json.loads(Path(sys.argv[1]).read_text());fail=[k for k,v in old.items() if new.get(k)!=v];print(json.dumps({'historicalChangedKeys':fail}));sys.exit(1 if fail else 0)
