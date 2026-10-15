// 只替换窗口组合：真实 Tiptap、草稿订阅与存储请求全部使用生产模块。
import {createRoot} from 'react-dom/client';
import {InputArea} from '../../components/InputArea';
import {useStore} from '../../stores';
import {createLocalServerConnection} from '../../services/server-connection';
import {hydrateInputDrafts,initInputDraftPersistence} from '../../stores/input-draft-persistence';
import {ensureSession,createNewSession,loadSessions,switchSession} from '../../stores/session-actions';
const info=await window.hana.getServerConnectionInfo!();
const ids=await (window.hana as unknown as {getSyntheticSessions:()=>Promise<{sessionId:string;otherSessionId:string}>}).getSyntheticSessions();
const connection=createLocalServerConnection({serverPort:info.port,serverToken:info.token,serverNodeKind:'lingxi-service'})!;
useStore.setState({activeServerConnection:connection,activeServerConnectionId:connection.connectionId,serverConnections:{[connection.connectionId]:connection},currentSessionId:ids.sessionId,currentSessionPath:`native:${ids.sessionId}`,currentAgentId:'focus-b',sessions:[ids.sessionId,ids.otherSessionId].map(id=>({sessionId:id,path:`native:${id}`,agentId:'focus-b',title:id})) as never});
initInputDraftPersistence(); await hydrateInputDrafts();
(window as typeof window & {focusFixture:unknown}).focusFixture={store:useStore,ids,ensureSession,createNewSession,loadSessions,switchSession,hydrateInputDrafts};
createRoot(document.getElementById('react-root')!).render(<InputArea/>);
