// 仅供真实窗口初始化边界验收：用公开 React API 重开现役 SettingsApp，不替换业务读写。
import { createRoot, type Root } from 'react-dom/client';
import { SettingsApp } from '../SettingsApp';
import { initTheme, initDragPrevention } from '../../../react/bootstrap';
initTheme(); initDragPrevention();
let root: Root | null = null;
const surface = window as typeof window & { r06SettingsMount: () => void; r06SettingsUnmount: () => void };
surface.r06SettingsMount = () => { root = createRoot(document.getElementById('react-root')!); root.render(<SettingsApp />); };
surface.r06SettingsUnmount = () => { root?.unmount(); root = null; };
surface.r06SettingsMount();
