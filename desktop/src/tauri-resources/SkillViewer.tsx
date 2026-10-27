import { useEffect, useRef, useState } from 'react';
import type { ResourceBridge, SkillPayload, SkillTree } from './bridge';

export function NativeSkillViewer({ bridge }: { bridge: ResourceBridge }) {
  const [payload, setPayload] = useState<SkillPayload | null>(null);
  const [tree, setTree] = useState<SkillTree[]>([]);
  const [content, setContent] = useState<string | null>(null);
  const [selected, setSelected] = useState('');
  const [error, setError] = useState('');
  const generation = useRef(0);
  const read = async (path: string) => {
    const current = ++generation.current;
    setSelected(path); setContent(null);
    try { const value = await bridge.readSkillFile(path); if (current === generation.current) setContent(value); }
    catch (error) { if (current === generation.current) setError(String(error)); }
  };
  useEffect(() => {
    let alive = true;
    void bridge.skillViewerLoad().then(async payload => {
      if (!payload) throw new Error('没有此窗口的技能目录');
      const files = await bridge.listSkillFiles(payload.baseDir);
      if (!alive) return;
      setPayload(payload); setTree(files);
      await read(payload.filePath || payload.baseDir + '/SKILL.md');
    }).catch(error => { if (alive) setError(String(error)); });
    return () => { alive = false; generation.current++; };
  }, [bridge]);
  const rows = (files: SkillTree[]) => files.map(file => file.isDir
    ? <details key={file.path}><summary>{file.name}</summary>{rows(file.children)}</details>
    : <button key={file.path} data-path={file.path} aria-pressed={selected === file.path} onClick={() => void read(file.path)}>{file.name}</button>);
  return <main data-testid="native-skill-viewer"><header><h1>{payload?.name || '技能查看器'}</h1><button onClick={() => void bridge.skillViewerClose()}>关闭窗口</button></header>
    <aside aria-label="技能文件">{rows(tree)}</aside><section><h2>{selected.split(/[\\/]/).pop()}</h2>{error && <p role="alert">{error}</p>}
      {content === null ? <p data-testid="skill-empty">无法读取此文件</p> : <pre data-testid="skill-content">{content}</pre>}
    </section><footer>只读查看 · 不会安装或修改技能</footer></main>;
}
