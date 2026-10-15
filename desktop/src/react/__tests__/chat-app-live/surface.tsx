// 仅提供宿主连接与真实用户操作入口，页面、侧栏、预览及请求全部使用生产组件。
import { createRoot } from 'react-dom/client';
import { initErrorBusBridge } from '../../errors/error-bus-bridge';
import { AppPages } from '../../components/app/AppPages';
import { ToastContainer } from '../../components/ToastContainer';
import { useStore } from '../../stores';
import { connectWebSocket, getWebSocket, disconnectWebSocketForRestart } from '../../services/websocket';
import { createLocalServerConnection } from '../../services/server-connection';
import { readRustChatHistory } from '../../services/rust-chat-history';
import { loadModels } from '../../utils/ui-helpers';
import { openFilePreview } from '../../utils/file-preview';
import { openSelectionInSideChat } from '../../stores/side-chat-actions';
import { attachFilesFromPaths } from '../../MainContent';
import { sessionScopedValue, sessionScopedListIncludes } from '../../stores/session-slice';
initErrorBusBridge();
const info = await window.hana.getServerConnectionInfo!();
const connection = createLocalServerConnection({serverPort:info.port,serverToken:info.token,serverNodeKind:'lingxi-service'})!;
useStore.setState({activeServerConnection:connection,activeServerConnectionId:connection.connectionId,serverConnections:{[connection.connectionId]:connection},currentTab:'chat',currentSessionId:null,currentSessionPath:null,currentAgentId:'lingxi',welcomeVisible:true,pendingNewSession:false,agents:[]});
async function select(id:string){
 const path=`native:${id}`;
 useStore.setState(s=>({sessions:[...s.sessions.filter(v=>v.sessionId!==id),{sessionId:id,path,agentId:'lingxi',title:'实际页面验证'}] as never,currentSessionId:id,currentSessionPath:path,pendingNewSession:false,welcomeVisible:false}));
 const history=await readRustChatHistory(connection,id,{all:true});
 if(history.notModified)throw Error('initial history cannot be 304');
 useStore.getState().initSession(path,history.items,history.page.hasMore,history.revision,history.page.nextCursor);
 await loadModels();
}
Object.assign(window,{chatAppFixture:{store:useStore,connect:connectWebSocket,disconnect:disconnectWebSocketForRestart,getWebSocket,select,openFilePreview,attachFilesFromPaths,openSide:openSelectionInSideChat,streaming:(path:string)=>sessionScopedListIncludes(useStore.getState(),useStore.getState().streamingSessions,path),session:(path:string)=>sessionScopedValue(useStore.getState(),useStore.getState().chatSessions,path)}});
createRoot(document.getElementById('react-root')!).render(<><div className="app-shell"><div className="app"><AppPages/></div></div><ToastContainer/></>);
