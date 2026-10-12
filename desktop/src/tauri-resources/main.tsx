import { createRoot } from 'react-dom/client';
import { ResourceDesktop } from './App';
import { nativeBridge } from './bridge';
import './style.css';

const root = createRoot(document.getElementById('root')!);
try {
  root.render(<ResourceDesktop bridge={nativeBridge()} viewer={new URLSearchParams(location.search).has('viewer')} />);
} catch (error) {
  root.render(<p role="alert">无法启动文件界面：{String(error)}</p>);
}
