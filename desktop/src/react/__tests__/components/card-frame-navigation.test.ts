/** @vitest-environment node */
import { createRequire } from 'node:module';
import { EventEmitter } from 'node:events';
import { describe,it,expect,vi } from 'vitest';
const { installCardFrameNavigationGuard,isCardDocument }=createRequire(import.meta.url)('../../../shared/card-frame-navigation.cjs');
const url=`http://127.0.0.1:3210/lingxi/v1/cards/card1/frame/${'a'.repeat(64)}`;
describe('卡片脚本导航的宿主阻断',()=>{
 it('阻断自身、嵌套和其他目标外送，保留可信父页面更换票据',()=>{
  const contents=new EventEmitter();const onBlocked=vi.fn();const release=installCardFrameNavigationGuard(contents,onBlocked);
  const parent={url:'file:///app/index.html',parent:null};const frame={url,parent};
  function navigate(target:string,from:unknown=frame,to:unknown=frame){const preventDefault=vi.fn();contents.emit('will-frame-navigate',{url:target,frame:to,initiator:from,preventDefault});return preventDefault;}
  expect(navigate('http://127.0.0.1:5555/leak?ticket=x')).toHaveBeenCalled();
  expect(navigate('https://outside.invalid/',frame,parent)).toHaveBeenCalled();
  expect(navigate('http://127.0.0.1:5555/',{url:'about:blank',parent:frame})).toHaveBeenCalled();
  expect(navigate(url+'?leak=x')).toHaveBeenCalled();
  expect(navigate(url+'#hash')).not.toHaveBeenCalled();
  expect(navigate(url.replace('a'.repeat(64),'b'.repeat(64)),parent)).not.toHaveBeenCalled();
  expect(onBlocked).toHaveBeenCalledTimes(4);release();expect(contents.listenerCount('will-frame-navigate')).toBe(0);
 });
 it('卡片识别不将查询串或任意服务器路径当能力文档',()=>{expect(isCardDocument(url)).toBe(true);expect(isCardDocument('http://127.0.0.1/lingxi/v1/cards/card1')).toBe(false);expect(isCardDocument('file:///cards/x')).toBe(false);});
});
