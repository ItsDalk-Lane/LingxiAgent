// 仅组合入口和合成宿主配置；业务组件、读取、发送及状态均使用生产模块。
import { createRoot } from 'react-dom/client';
import { ToastContainer } from '../../components/ToastContainer';
import { ChatPage } from '../../components/app/ChatPage';
import { useStore } from '../../stores';
import { connectWebSocket, disconnectWebSocketForRestart, getWebSocket } from '../../services/websocket';
import { createLocalServerConnection } from '../../services/server-connection';
import { readRustChatHistory } from '../../services/rust-chat-history';
import { loadModels } from '../../utils/ui-helpers';
import { sessionScopedValue, sessionScopedListIncludes } from '../../stores/session-slice';
import { getDesktopHost } from '../../host/desktop-host';
const info = await getDesktopHost().api.getServerConnectionInfo!();
const connection = createLocalServerConnection({ serverPort: info.port, serverToken: info.token, serverNodeKind: 'lingxi-service' })!;
useStore.setState({activeServerConnection:connection,activeServerConnectionId:connection.connectionId,serverConnections:{[connection.connectionId]:connection},currentSessionId:null,currentSessionPath:null,currentAgentId:'lingxi',welcomeVisible:true,pendingNewSession:false,agents:[]});
async function select(sessionId:string) {
 const path=`native:${sessionId}`;
 useStore.setState({sessions:[{sessionId,path,agentId:'lingxi',title:'真实聊天验证'}] as never,currentSessionId:sessionId,currentSessionPath:path,welcomeVisible:false,pendingNewSession:false});
 const history=await readRustChatHistory(connection,sessionId,{all:true});
 if(history.notModified)throw Error('initial history cannot be 304');
 useStore.getState().initSession(path,history.items,history.page.hasMore,history.revision,history.page.nextCursor);
 await loadModels();
 return history;
}
const root=createRoot(document.getElementById('react-root')!);
const fixture={isStreaming:()=>sessionScopedListIncludes(useStore.getState(),useStore.getState().streamingSessions,'native:desktop'),surface:(inputSurface:'desktop'|'mobile')=>root.render(<><ChatPage inputSurface={inputSurface}/><ToastContainer/></>),loadModels,pending:()=>useStore.setState({currentSessionId:null,currentSessionPath:null,pendingNewSession:true,welcomeVisible:true}),store:useStore,connect:connectWebSocket,disconnect:disconnectWebSocketForRestart,getWebSocket,select,history:()=>readRustChatHistory(connection,'desktop',{all:true}),conditionalHistory:(etag:string)=>readRustChatHistory(connection,'desktop',{etag}),pagedHistory:(cursor?:string)=>readRustChatHistory(connection,'desktop',{limit:1,cursor}),session:()=>sessionScopedValue(useStore.getState(),useStore.getState().chatSessions,'native:desktop')};
Object.assign(window,{chatFixture:fixture});
root.render(<><ChatPage /><ToastContainer/></>);
