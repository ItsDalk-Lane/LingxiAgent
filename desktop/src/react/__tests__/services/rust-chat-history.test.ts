import { describe, expect, it } from 'vitest';
import { projectRustHistoryItems } from '../../services/rust-chat-history';
const run={state:'known',runId:'r'};
const user={kind:'user_message',messageId:'u',seq:'9007199254740993',committedAtUnixMs:1,text:'hello',run};
const segment={kind:'assistant_segment',segmentId:'s',run,phase:'unresolved',segmentKind:'text',text:'partial',complete:false,firstSeq:'9007199254740994'};
describe('Rust canonical history shares the production turn projector',()=>{
 it('never seals an unresolved partial or invents a terminal from an empty tail',()=>{
  const items=projectRustHistoryItems([user,segment],'session');
  const item=items[1];expect(item.type).toBe('message');if(item.type!=='message')throw Error();
  expect(item.data.turnProjection?.status).toBe('streaming');expect(item.data.turnProjection?.outcome).toBeUndefined();
  expect(item.data.blocks).toEqual(expect.arrayContaining([expect.objectContaining({source:'partial',surfaceRole:'provisional',lifecycle:'streaming'})]));
  expect(item.data.blocks?.some(v=>v.type==='turn_status')).toBe(false);
 });
 it('preserves a real cancelled terminal and does not manufacture a final reply',()=>{
  const items=projectRustHistoryItems([user,segment,{kind:'run_terminal',runId:'r',status:'cancelled',eventSeq:'9007199254740995'}],'session');
  const item=items[1];if(item.type!=='message')throw Error();expect(item.data.turnProjection?.status).toBe('aborted');expect(item.data.turnProjection?.outcome).toBe('aborted');expect(item.data.sourceEntryId).toBeUndefined();
 });
 it('deduplicates streamed final against the actual final and retains opaque state as an explicit notice',()=>{
  const final={kind:'final_message',messageId:'f',seq:'9007199254740995',run,content:[{type:'text',text:'answer'},{type:'opaque',provider:'p',data:{signature:'original-opaque'}}],committedAtUnixMs:2};
  const input=[user,{...segment,phase:'final_answer',text:'answer',complete:true},final];const before=JSON.stringify(input);
  const items=projectRustHistoryItems(input,'session');const item=items[1];if(item.type!=='message')throw Error();
  expect(item.data.blocks?.filter(v=>v.type==='text').map(v=>v.source)).toEqual(['answer']);expect(item.data.blocks?.some(v=>v.type==='interlude'&&v.text.includes('专用状态已原样保留'))).toBe(true);expect(JSON.stringify(input)).toBe(before);expect(item.data.sourceEntryId).toBe('f');expect(item.data.turnProjection?.outcome).toBe('completed_with_answer');
 });
 it('never assigns an unknown run record to the neighbouring visible user',()=>{
  const items=projectRustHistoryItems([user,{...segment,run:{state:'unknown',reason:'fork'},complete:true}],'session');const item=items[1];if(item.type!=='message')throw Error();expect(item.data.turnInputEntryId).toBeUndefined();expect(item.data.turnProjection?.inputMessageId).toBeNull();
 });
 it('keeps started and unknown tool outcomes distinct from success and failure',()=>{
  const input=[user,...['started','unknown','success'].map((status,i)=>({kind:'tool_call',run,toolCallId:`t${i}`,target:'read',status,firstSeq:String(i+2)}))];
  const item=projectRustHistoryItems(input,'session')[1];if(item.type!=='message')throw Error();const tools=item.data.blocks?.flatMap(v=>v.type==='tool_group'?v.tools:[])||[];
  expect(tools.map(v=>({done:v.done,success:v.success,status:v.status}))).toEqual([{done:false,success:false,status:'running'},{done:true,success:false,status:'unknown'},{done:true,success:true,status:'succeeded'}]);
 });
 it('marks legacy archives explicitly and rejects unknown future schema rather than returning an empty success',()=>{
  const items=projectRustHistoryItems([{kind:'legacy_raw',messageId:'old',seq:'1',raw:{retained:true},reason:'unknown original'}],'session');expect(items[0].type).toBe('interlude');expect(()=>projectRustHistoryItems([{kind:'made_up'}],'session')).toThrow('尚不支持');
 });
});


describe('Rust user presentation retains the real scoped source', () => {
 it('renders saved user text safely and retains quoted text and optional-label session references', () => {
  const result=projectRustHistoryItems([{...user,text:'<img src=x onerror=alert(1)>',legacyFields:{quotedText:'真实引文',sessionRefs:[{sessionId:'source'}]}}],'session');
  expect(result[0].type).toBe('message');if(result[0].type!=='message')throw Error();
  expect(result[0].data.textHtml).toContain('&lt;img');expect(result[0].data.textHtml).not.toContain('<img');
  expect(result[0].data.quotedText).toBe('真实引文');expect(result[0].data.sessionRefs).toEqual([{sessionId:'source',label:'Session'}]);
 });
});
