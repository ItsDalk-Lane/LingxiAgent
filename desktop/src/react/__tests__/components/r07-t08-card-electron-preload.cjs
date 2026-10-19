const { contextBridge } = require('electron');
// 仅测试窗口的连接桥，地址、凭证和卡片全部由真实Rust测试传入。
contextBridge.exposeInMainWorld('cardFixture', { config: { url: process.env.LINGXI_R07_CARD_MOBILE_CSP === '1' ? new URL(process.env.LINGXI_R07_CARD_RENDERER).origin : process.env.LINGXI_R07_CARD_URL, token: process.env.LINGXI_R07_CARD_TOKEN, block: JSON.parse(process.env.LINGXI_R07_CARD_BLOCK) } });

if(process.env.LINGXI_R07_CARD_GUARD==='1') contextBridge.exposeInMainWorld('hana',{cardFrameNavigationGuard:true});
