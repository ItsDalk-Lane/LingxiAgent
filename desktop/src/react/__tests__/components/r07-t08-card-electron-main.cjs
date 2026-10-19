const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const home = process.env.LINGXI_R07_CARD_HOME;
app.setPath('userData', path.join(home, '.card-electron-profile'));
app.commandLine.appendSwitch('disable-gpu');
let window;
const checks = [];
const requests = [];
const blockedNavigations=[]; const navigationFailures=[]; const cspMessages=[];
async function until(fn, message) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) { try { const value = await fn(); if (value) return value; } catch {} await new Promise(resolve => setTimeout(resolve, 25)); }
  throw new Error(message);
}
app.whenReady().then(async () => {
  window = new BrowserWindow({ show:!!process.env.LINGXI_R07_CARD_COLLECTOR, width:900, height:700, webPreferences:{backgroundThrottling:false,contextIsolation:true,nodeIntegration:false,preload:path.join(__dirname,'r07-t08-card-electron-preload.cjs')} });
  if(process.env.LINGXI_R07_CARD_GUARD==='1') require('../../../shared/card-frame-navigation.cjs').installCardFrameNavigationGuard(window.webContents, detail=>blockedNavigations.push(detail));
  window.webContents.on('console-message',(event,_level,message)=>{const text=event.message||message||'';if(text.includes('frame-src')||text.includes('Framing')) cspMessages.push(text.replace(/http[^ ]+/g,'[URL]'));});
  window.webContents.on('did-fail-load',(_event,code,description)=>navigationFailures.push({code,description}));
  const actualOrigin=process.env.LINGXI_R07_CARD_MOBILE_CSP==='1'?new URL(process.env.LINGXI_R07_CARD_RENDERER).origin:process.env.LINGXI_R07_CARD_URL;
  window.webContents.session.webRequest.onCompleted(details => { if (details.url.startsWith(actualOrigin)) requests.push({method:details.method,path:new URL(details.url).pathname,status:details.statusCode}); });
  await window.loadURL(process.env.LINGXI_R07_CARD_RENDERER);
  const frame = await until(async () => window.webContents.mainFrame.framesInSubtree.find(f=>f.url.startsWith(actualOrigin+'/lingxi/v1/cards/')), '实际组件没有加载真实服务iframe');
  await until(async () => frame.executeJavaScript('!!document.querySelector("#actual-card")'), '实际iframe内容不存在');
  if(!process.env.LINGXI_R07_CARD_BASELINE_SOURCE) { assert(!frame.url.includes(process.env.LINGXI_R07_CARD_TOKEN), '框架地址不能包含整机凭证'); checks.push('actual-frame-no-owner-token'); }
  assert(requests.some(r=>r.method==='PUT'&&r.status===200));assert(requests.some(r=>r.method==='GET'&&r.status===200));checks.push('actual-component-put-and-get');
  const a = await window.webContents.executeJavaScript('(()=>{const r=document.querySelector("iframe").getBoundingClientRect();return {x:r.x,y:r.y}})()');
  const b = await frame.executeJavaScript('(()=>{const r=document.querySelector("#actual-card").getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()');
  if (process.env.LINGXI_R07_CARD_COLLECTOR) { window.show(); window.focus(); window.webContents.focus(); }
  window.webContents.sendInputEvent({type:'mouseDown',x:Math.round(a.x+b.x),y:Math.round(a.y+b.y),button:'left',clickCount:1});
  window.webContents.sendInputEvent({type:'mouseUp',x:Math.round(a.x+b.x),y:Math.round(a.y+b.y),button:'left',clickCount:1});
  if (process.env.LINGXI_R07_CARD_COLLECTOR) {
    const collector = process.env.LINGXI_R07_CARD_COLLECTOR;
    console.log('CARD_CLICK_STATE:'+JSON.stringify(await frame.executeJavaScript('({text:document.querySelector("#actual-card")?.textContent,hasHandler:!!document.querySelector("#actual-card")?.onclick,tokenReadable:location.search.includes("token=")})')));
    // 诊断验证卡片脚本本身的导航能力，不将未命中的原生鼠标事件冒充用户验收。
    await frame.executeJavaScript('document.querySelector("#actual-card")?.click()');
    if(process.env.LINGXI_R07_CARD_EXPECT_BLOCK==='1') {
      await until(()=>blockedNavigations.length||cspMessages.length, '没有真实宿主或父CSP阻断证据');
      if(blockedNavigations.length) assert(await frame.executeJavaScript('document.querySelector("#actual-card")?.textContent==="done"'));
      else assert(navigationFailures.some(item=>item.code===-30), '父CSP应给出真实ERR_BLOCKED_BY_CSP');
      const outcome=await fetch(collector+'/proof').then(r=>r.json());assert.equal(outcome.count,0);
      const report={checks:[...checks,'actual-script-executed','actual-self-navigation-blocked'],requests,outcome,blockedNavigations,cspMessages,navigationFailures};fs.writeFileSync(path.join(home,'card-native-result.json'),JSON.stringify(report,null,2));console.log('T08_CARD_NATIVE_RESULT:'+JSON.stringify(report));window.destroy();app.exit(0);return;
    }
    const outcome = await until(async () => { const result = await fetch(collector+'/proof').then(r=>r.json()); return result.count ? result : null; }, '自导航未到达本机收集器');
    const report={checks:[...checks,'actual-self-navigation-loopback-observed'],requests,outcome};
    fs.writeFileSync(path.join(home,'card-native-result.json'),JSON.stringify(report,null,2));console.log('T08_CARD_NATIVE_RESULT:'+JSON.stringify(report));window.destroy();app.exit(0);return;
  }
  await frame.executeJavaScript('document.querySelector("#actual-card")?.click()');
  await until(async ()=>frame.executeJavaScript('document.querySelector("#actual-card").textContent==="done"'),'真实按钮点击无效');checks.push('actual-frame-click');
  const blocked = await frame.executeJavaScript(`new Promise(resolve=>{fetch(${JSON.stringify(process.env.LINGXI_R07_CARD_URL+'/must-not-connect')}).then(()=>resolve(false),()=>resolve(true));})`);
  assert.equal(blocked,true);assert(!requests.some(r=>r.path==='/must-not-connect'));checks.push('actual-csp-connect-blocked');
  assert(await window.webContents.executeJavaScript('document.querySelector("iframe").getAttribute("sandbox")==="allow-scripts"'));checks.push('actual-opaque-frame-sandbox');
  const report={checks,requests};fs.writeFileSync(path.join(home,'card-native-result.json'),JSON.stringify(report,null,2));console.log('T08_CARD_NATIVE_RESULT:'+JSON.stringify(report));
  window.destroy();app.exit(0);
}).catch(error=>{console.error(error);if(window&&!window.isDestroyed())window.destroy();app.exit(1);});
