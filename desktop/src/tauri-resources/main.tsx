import { createRoot } from 'react-dom/client';
import { ResourceDesktop } from './App';
import { NativeSkillViewer } from './SkillViewer';
import { nativeBridge } from './bridge';
import './style.css';

const root = createRoot(document.getElementById('root')!);
try {
  const bridge = nativeBridge();
  root.render(new URLSearchParams(location.search).has('skillViewer') ? <NativeSkillViewer bridge={bridge} /> : <ResourceDesktop bridge={bridge} viewer={new URLSearchParams(location.search).has('viewer')} />);
} catch (error) {
  root.render(<p role="alert">无法启动文件界面：{String(error)}</p>);
}
