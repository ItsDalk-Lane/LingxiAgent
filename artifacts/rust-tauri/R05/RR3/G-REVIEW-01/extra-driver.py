import pathlib,json
E0=pathlib.Path(__file__).resolve().parent
exec((E0/'run-supplements.py').read_text().split('while not (E/')[0])
commands=json.loads((E/'supplement-commands.json').read_text())
exec((E/'run-extra-registry.py').read_text())
run('N16-used-root-refusal-supplement',[cargo,'run','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--','verify-stage','R02','--evidence',str(E/'default16-01/n16-binding-drift/run-a')])
extra_registry(run,C,E,cargo)
run('extra-registry-restored-all',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--bin','xtask','r05_'])
cred=C/'rust/crates/lingxi-service/src/credentials/mod.rs';old=cred.read_bytes();needle=b'assert!(text.contains("main"));';assert old.count(needle)==1
cred.write_bytes(old.replace(needle,b'assert!(false, "N04 injected failure");',1))
try:
 run('N04-direct-target-red',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--lib','credentials::tests::handle_refusal_texts_carry_no_material','--','--exact'])
finally:cred.write_bytes(old)
run('N04-direct-target-restored',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--lib','credentials::tests::handle_refusal_texts_carry_no_material','--','--exact'])
run('I06-existing-approval',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','r04_t03_approval_service'])
run('I06-existing-worker-permission',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','r04_t07_mcp_and_workers','mcp_and_worker_tools_follow_the_t03_permission_face','--','--exact'])
save(E/'extra-complete.json',{'utc':utc(),'files':snap(),'originalInputsEqual':snap()==json.loads((E/'copy-restored-inputs.json').read_text())['files'],'commands':commands})
