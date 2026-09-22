from pathlib import Path
import hashlib,json,subprocess
root=Path.cwd()
files=['package-lock.json','desktop/src/react/services/stream-admission.ts','desktop/src/react/services/stream-resume.ts','desktop/src/react/services/ws-message-handler.ts','desktop/src/react/hooks/use-stream-buffer.ts','tests/stream-route-consumer-isolation.test.ts','artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts']
print(json.dumps({'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'node':subprocess.check_output(['node','-v'],text=True).strip(),'sha256':{f:hashlib.sha256(Path(f).read_bytes()).hexdigest() for f in files}},ensure_ascii=False,indent=2))
for file,terms in [('server/routes/chat.ts',['function emitStreamEvent','一个用户 Run','复用 streamId','function splitAssistantRunForSteeredInput']),('server/routes/channels.ts',['buildConversationMarkdownExport','export']),('lib/channels/conversation-export.ts',['export async function','Conversation record','ensureTrailingNewline(content)'])]:
 print('\nSOURCE',file)
 for i,line in enumerate(Path(file).read_text().splitlines(),1):
  if any(term in line for term in terms):print(f'{i}: {line}')
print('\nDESKTOP STRUCTURED EXPORT ROUTE SEARCH')
for directory in ['desktop/src/react','server/routes']:
 hits=[]
 for p in Path(directory).rglob('*.ts*'):
  for i,line in enumerate(p.read_text().splitlines(),1):
   if any(term in line for term in ['exportSession','exportChat','exportMessages','导出会话','导出对话']):hits.append(f'{p}:{i}: {line}')
 print(directory, json.dumps(hits,ensure_ascii=False))
