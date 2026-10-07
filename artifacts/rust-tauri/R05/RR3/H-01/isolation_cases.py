from capture import *
import shutil

iso=EV/'isolated'
label=sys.argv[1]
main=iso/'rust/crates/lingxi-service/src/main.rs'
redaction=iso/'rust/crates/lingxi-service/src/redaction.rs'
logging=iso/'rust/crates/lingxi-service/src/logging.rs'
paths=[main,redaction,logging,iso/'rust/crates/lingxi-service/src/lib.rs']
for p in paths:
    # 写入新时间戳，让 Cargo 检出字节还原；保留旧 mtime 会复用变异产物。
    shutil.copyfile(ROOT/p.relative_to(iso),p)
original={str(p.relative_to(iso)):sha(p) for p in paths}
def replace_once(p,old,new):
    s=p.read_text();assert s.count(old)==1,(p,old,s.count(old));p.write_text(s.replace(old,new))
if label=='f48-boundary-red':
    replace_once(redaction,'write!(f, "{:?}", self.0)','f.write_str(self.0)')
elif label=='f47-root-red':
    replace_once(main,'let config = match ServiceConfig::from_sources(', 'let mut config = match ServiceConfig::from_sources(')
    anchor='    // 证书/私钥错误必须在绑定端口和写入实例记录之前暴露。'
    replace_once(main,anchor,'    if config.data_home.join("data-epoch.json").is_file() {\n        if let Some(fallback) = env_home.as_deref() {\n            config.data_home = std::path::PathBuf::from(fallback);\n        }\n    }\n'+anchor)
elif label=='f47-guard-red':
    replace_once(main,'"data-epoch gate refused startup (exit 2)"\n                );\n                return ExitCode::from(2);', '"data-epoch gate refused startup (exit 2)"\n                );\n                return ExitCode::SUCCESS;')
elif label=='f46-old-prune-red':
    replace_once(logging,'log.open_current()?;\n        log.prune()?;', 'log.prune()?;\n        log.open_current()?;')
else:
    assert label in ['f48-restored-green','f47-restored-green','f47-restored-green-2','f46-restored-green']
write(EV/(label+'-mutation.json'),{'UTC':utc(),'source':str(iso),'original':original,'mutated':{str(p.relative_to(iso)):sha(p) for p in paths},'boundary':'隔离源码；实际Cargo构建复用主缓存，但主源码不改；不操作G COPY或NEG_TARGET'})
if label.startswith('f48'):
    argv=['bash',str(iso/'scripts/rust-tauri/r02_t07_redaction_scan.sh'),str(EV/'commands'/label/'A13')]
elif label.startswith('f47'):
    argv=['bash',str(iso/'scripts/rust-tauri/r02_t01_service_smoke.sh'),str(EV/'commands'/label/'A01')]
else:
    argv=['/Users/study_superior/.cargo/bin/cargo','build','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service']
code=run(label,argv,{'RUST_LOG':'warn'},cwd=iso)
write(EV/(label+'-source-after.json'),{str(p.relative_to(iso)):sha(p) for p in paths})
sys.exit(code)
