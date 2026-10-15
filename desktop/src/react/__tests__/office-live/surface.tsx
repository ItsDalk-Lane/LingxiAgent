// 使用当前预览入口、页面和错误提示，只替换窗口组合入口。
import { createRoot } from 'react-dom/client';
import { PreviewPanel } from '../../components/PreviewPanel';
import { ToastContainer } from '../../components/ToastContainer';
import { openFilePreview } from '../../utils/file-preview';
import { useStore } from '../../stores';
import { initErrorBusBridge } from '../../errors/error-bus-bridge';
initErrorBusBridge();
const surface=window as typeof window & { officeFixture: { open:typeof openFilePreview; store:typeof useStore } };
surface.officeFixture={open:openFilePreview,store:useStore};
createRoot(document.getElementById('react-root')!).render(<><PreviewPanel/><ToastContainer/></>);
