// 连接、入帧判断和列表更新全部调用生产模块。
import {createRoot} from 'react-dom/client';
import {useStore} from '../../stores';
import {createLocalServerConnection} from '../../services/server-connection';
import {connectWebSocket} from '../../services/websocket';
const info=await window.hana.getServerConnectionInfo!();
const connection=createLocalServerConnection({serverPort:info.port,serverToken:info.token,serverNodeKind:'lingxi-service'})!;
useStore.setState({activeServerConnection:connection,activeServerConnectionId:connection.connectionId,serverConnections:{[connection.connectionId]:connection},sessions:[],sessionLocatorsById:{}});
Object.assign(window,{sessionCreatedFixture:{store:useStore,connect:connectWebSocket}});
function Result(){const sessions=useStore(s=>s.sessions);return <div>{sessions.map(s=><span key={s.sessionId}>{s.sessionId}</span>)}</div>}
createRoot(document.getElementById('react-root')!).render(<Result/>);
