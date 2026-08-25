import { lazy, Suspense } from 'react';
import { useStore } from '../../stores';
import { InputArea, type InputAreaProps } from '../InputArea';
import { WelcomeScreen } from '../WelcomeScreen';
import { ChatArea } from '../chat/ChatArea';
import { RegionalErrorBoundary } from '../RegionalErrorBoundary';

// 运行信息胶囊仅桌面 chat 使用；懒加载避免移动端主包吃进运行卡模块树
const LazyRuntimeInfoCapsule = lazy(() =>
  import('../runtime/RuntimeInfoCapsule').then(m => ({ default: m.RuntimeInfoCapsule })),
);

function WelcomeContainer() {
  const visible = useStore(s => s.welcomeVisible);
  return (
    <div className={`welcome${visible ? '' : ' hidden'}`} id="welcome">
      <WelcomeScreen />
    </div>
  );
}

export function ChatPage({
  inputSurface = 'desktop',
  regionPrefix = '',
}: {
  inputSurface?: NonNullable<InputAreaProps['surface']>;
  regionPrefix?: string;
} = {}) {
  const welcomeVisible = useStore(s => s.welcomeVisible);
  const currentSessionPath = useStore(s => s.currentSessionPath);
  const hasPanels = !welcomeVisible && !!currentSessionPath;

  return (
    <>
      {inputSurface === 'desktop' && (
        <Suspense fallback={null}>
          <LazyRuntimeInfoCapsule />
        </Suspense>
      )}
      <div className={`chat-area${hasPanels ? ' has-panels' : ''}`}>
        <WelcomeContainer />
        <RegionalErrorBoundary region={`${regionPrefix}chat`} resetKeys={[currentSessionPath]}>
          <ChatArea />
        </RegionalErrorBoundary>
      </div>
      <div className="input-area">
        <RegionalErrorBoundary
          region={`${regionPrefix}input`}
          resetKeys={[currentSessionPath]}
          autoRetry={inputSurface === 'mobile' ? { attempts: 2, delayMs: 120 } : undefined}
        >
          <InputArea key={currentSessionPath || '__new'} surface={inputSurface} />
        </RegionalErrorBoundary>
      </div>
    </>
  );
}
