from pathlib import Path
import concurrent.futures,json,os,subprocess
root=Path(__file__).resolve().parent
cache=Path('/Users/study_superior/.codex/r06-w05-r01-runtime/target/debug/deps')
checks=[
('F01','r06_ar_f01_history','f01_c01_real_authenticated_history_and_isolation'),
('F05','r06_ar_f05_cache','c01_actual_calls_keep_system_frozen_and_tools_live_after_description_update'),
('F11','r06_ar_w11_embedding_lifecycle','c03_actual_driver_embedding_network_deadline_is_not_renewed'),
('F12','r06_ar_f12_cache_preservation','c01_future_schema_preserves_sentinel_in_both_caches'),
('F13','r06_ar_f13_submission','real_submission_freezes_selected_sources_and_rejects_old_or_foreign_scope'),
]
def run(item):
 issue,suite,name=item
 b=max((p for p in cache.glob(suite+'-*') if p.is_file() and os.access(p,os.X_OK)),key=lambda p:p.stat().st_mtime)
 env=os.environ.copy();home=root/'synthetic-home'/issue;home.mkdir(parents=True,exist_ok=True)
 evidence=root/'direct-probes'/(issue+'-final');evidence.mkdir(parents=True,exist_ok=True)
 env.update(LINGXI_HOME=str(home),TEST_TIME_LIMIT_SEC='90',W01_EVIDENCE=str(evidence),LINGXI_F05_EVIDENCE=str(evidence),W11_EVIDENCE=str(evidence))
 p=subprocess.run([str(b),name,'--exact','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
 (evidence/'check.log').write_text(p.stdout)
 r={'issue':issue,'suite':suite,'test':name,'exitCode':p.returncode,'log':str(evidence/'check.log')}
 print(json.dumps(r),flush=True)
 if p.returncode:print('\n'.join(p.stdout.splitlines()[-35:]),flush=True)
 return r
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as ex:results=list(ex.map(run,checks))
(root/'final-probe-results.json').write_text(json.dumps(results,indent=2)+'\n')
