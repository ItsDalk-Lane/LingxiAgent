from capture import *
iso=EV/'isolated'
def case(label,expected):
 code=run(label+'-driver',['python3',str(EV/'isolation_cases.py'),label]);assert code==expected,(label,code)
case('f47-restored-green',0)
case('f47-guard-red',1)
case('f47-restored-green-2',0)
case('f47-root-red',1)
# 每次原字节恢复使用新时间戳，检查实际重新编译。
case('f48-restored-green',0)
case('f48-boundary-red',1)
code=run('f48-boundary-unit-red',['/Users/study_superior/.cargo/bin/cargo','test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--lib','redaction::tests::f48_','--','--nocapture'],cwd=iso);assert code==101,code
# 另一个有效恢复编号，避免覆盖旧证据。
restore=EV/'isolation_cases.py';s=restore.read_text();s=s.replace("'f48-restored-green','f47-restored-green'","'f48-restored-green','f48-restored-green-2','f47-restored-green'");restore.write_text(s)
case('f48-restored-green-2',0)
case('f46-old-prune-red',0)
code=run('f46-old-six-command',['python3',str(EV/'binary_probe.py'),'f46-old-six',str(ROOT/'rust/target/debug/lingxi-service'),'six']);assert code==1,code
case('f46-restored-green',0)
code=run('f46-restored-six-command',['python3',str(EV/'binary_probe.py'),'f46-restored-six',str(ROOT/'rust/target/debug/lingxi-service'),'six']);assert code==0,code
print('隔离全部目标红与实际恢复绿完成',flush=True)
