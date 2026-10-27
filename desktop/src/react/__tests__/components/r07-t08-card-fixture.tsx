
import React from 'react';
import { createRoot } from 'react-dom/client';
import { useStore } from '../../stores';
import { captureSessionConnection } from '../../services/session-connection-owner';
import type { ServerConnection } from '../../services/server-connection';
declare global { interface Window { cardFixture: { config: { url:string; token:string; block:{cardId:string;code:string;title:string} } } } }
import { InteractiveCard } from '../../components/chat/InteractiveCard';
const config = window.cardFixture.config;
window.t = key => key;
const connection:ServerConnection = { connectionId:'t08-real-card', kind:'local', serverId:'local', serverNodeKind:'lingxi-service', studioId:'local', label:'真实卡片服务', baseUrl:config.url, wsUrl:config.url.replace('http:','ws:'), token:config.token, authState:'paired', trustState:'local', credentialKind:'loopback_token', capabilities:[] };
useStore.setState({ serverConnections:{ [connection.connectionId]:connection }, activeServerConnectionId:connection.connectionId, activeServerConnection:connection });
captureSessionConnection();
createRoot(document.getElementById('root')!).render(React.createElement(InteractiveCard,{block:{type:'interactive_card',...config.block}}));
