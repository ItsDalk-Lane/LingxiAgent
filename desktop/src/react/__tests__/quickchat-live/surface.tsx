// 合成宿主页只选择生产组件；发送、会话切换和跨窗口确认均用现役代码。
import {createRoot} from 'react-dom/client';
import {QuickChatApp} from '../../quick-chat/QuickChatApp';
import {ChatPage} from '../../components/app/ChatPage';
import {ToastContainer} from '../../components/ToastContainer';
import {useStore} from '../../stores';
import {createLocalServerConnection} from '../../services/server-connection';
import {openQuickChatSession,quickChatOwnerKey} from '../../services/quick-chat-session-handoff';
import {switchSession} from '../../stores/session-actions';
import {connectWebSocket,getWebSocket} from '../../services/websocket';
import zh from '../../../locales/zh.json';
const translate=(key:string)=>key.split('.').reduce<any>((v,k)=>v?.[k],zh)??key;
window.t=translate as never;window.i18n={locale:'zh-CN',load:async()=>{}} as never;window.platform=window.hana;
const quick=new URLSearchParams(location.search).get('surface')==='quick';
const info=await window.hana.getServerConnectionInfo!();
const connection=createLocalServerConnection({serverPort:info.port,serverToken:info.token,serverNodeKind:info.serverNodeKind,serverNodeTransport:info.serverNodeTransport})!;
const fixture={store:useStore,getWebSocket,delay:0,quickChatOwnerKey,connection};Object.assign(window,{quickChatFixture:fixture});
if(!quick){
 useStore.setState({serverPort:String(info.port),serverToken:info.token,serverConnections:{local:connection},activeServerConnectionId:'local',activeServerConnection:connection,currentAgentId:'quick-a',pendingNewSession:false,welcomeVisible:true});
 const agents=await fetch(connection.baseUrl+'/lingxi/v1/agents',{headers:{Authorization:`Bearer ${connection.token}`}}).then(r=>r.json());useStore.setState({agents:agents.agents});
 window.hana.onQuickChatOpenSession?.(async request=>{const result=await openQuickChatSession(request);if(fixture.delay)await new Promise(resolve=>setTimeout(resolve,fixture.delay));return result;});
 await switchSession('native:quick-existing');connectWebSocket();
}
createRoot(document.getElementById('react-root')!).render(quick?<QuickChatApp/>:<main className="main-content" style={{height:"100%"}}><ChatPage/><ToastContainer/></main>);
