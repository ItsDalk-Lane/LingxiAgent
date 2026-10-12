import { useEffect, useRef, useState } from 'react';
import { markdownHtml, readPreview, retainResourceEvents, saveCover, type Preview } from './content';
import type { ResourceBridge, Snapshot } from './bridge';

export function ResourceDesktop({ bridge, viewer = false }: { bridge: ResourceBridge; viewer?: boolean }) {
  const [files, setFiles] = useState<string[]>([]);
  const [path, setPath] = useState('');
  const [preview, setPreview] = useState<Preview | null>(null);
  const [editor, setEditor] = useState('');
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [watchError, setWatchError] = useState('');
  const [destination, setDestination] = useState('');
  const [binaryData, setBinaryData] = useState('');
  const [cover, setCover] = useState('');
  const [openPath, setOpenPath] = useState('');
  const dirty = useRef(false);
  const baseline = useRef<Snapshot | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    if (!viewer) return;
    bridge.viewerRequestLoad().then(payload => {
      if (!payload?.readOnly) throw new Error('没有此窗口的只读文件信息');
      document.title = `${payload.title} · 只读`;
      setPath(payload.filePath);
    }).catch(e => setError(String(e)));
  }, [bridge, viewer]);

  useEffect(() => {
    const request = ++generation.current;
    setPreview(null); setError(''); setWatchError(''); dirty.current = false; baseline.current = null;
    if (!path) return;
    let disposed = false;
    const reload = async () => {
      try {
        const next = await readPreview(path, bridge);
        if (disposed || request !== generation.current) return;
        setPreview(next); setError('');
        if (!dirty.current || viewer) {
          baseline.current = next.snapshot || null;
          setEditor(next.content);
        } else setNotice('文件已被外部修改。尚未保存的编辑仍保留，保存时会核对版本。');
      } catch (e) { if (!disposed) setError(String(e)); }
    };
    void reload();
    const watch = retainResourceEvents(path, bridge, reload, e => { if (!disposed) setWatchError(String(e)); });
    watch.ready.catch(e => { if (!disposed) setWatchError(String(e)); });
    return () => { disposed = true; watch.release(); };
  }, [path, bridge, viewer]);

  const action = async (operation: () => Promise<void>) => {
    setError(''); setNotice('');
    try { await operation(); } catch (e) { setError(String(e)); }
  };
  const select = () => action(async () => {
    const chosen = await bridge.selectFiles();
    setFiles(previous => Array.from(new Set([...previous, ...chosen])));
    if (chosen.length) setNotice('已选择文件。点文件名后可查看；选择本身不会写入文件。');
  });
  const save = () => action(async () => {
    if (!baseline.current) throw new Error('文件没有可用的编辑版本');
    const saved = await bridge.writeFileIfUnchanged(path, editor, baseline.current.version);
    if (!saved.ok || !saved.version) throw new Error(saved.conflict ? '保存失败：文件已被外部修改，外部内容已保留。' : '保存失败，未显示保存成功。');
    dirty.current = false;
    baseline.current = { content: editor, version: saved.version };
    setPreview(await readPreview(path, bridge)); setNotice('已保存');
  });
  const changeCover = (image: string | null) => action(async () => {
    if (!baseline.current || dirty.current) throw new Error('请先保存或重新读取当前编辑，再修改封面');
    const { result, content } = await saveCover(path, baseline.current, image, bridge);
    if (!result.ok || !result.version) throw new Error('封面保存失败：文件可能已被外部修改，外部内容已保留。');
    baseline.current = { content, version: result.version }; setEditor(content);
    setPreview(await readPreview(path, bridge)); setNotice('封面已保存');
  });

  return <main data-testid={viewer ? 'readonly-viewer' : 'resource-desktop'}>
    <header><h1>{viewer ? '只读查看' : '灵犀 · 文件'}</h1><span>本地文件</span>
      {viewer ? <button onClick={() => void bridge.viewerClose()}>关闭窗口</button> : <button data-testid="select-files" onClick={select}>选择文件</button>}
    </header>
    {!viewer && <aside aria-label="文件列表">{files.length === 0 ? <p>先选择文件，再点文件名查看。</p> : files.map(file => <button key={file} data-path={file} className={path === file ? 'selected' : ''} onClick={() => setPath(file)}>{file.split(/[\\/]/).pop()}<small>{file}</small></button>)}</aside>}
    <section>
      <h2 data-testid="file-path">{path || '尚未打开文件'}</h2>
      {!viewer && <div className="inline"><input aria-label="本地文件路径" value={openPath} onChange={e => setOpenPath(e.target.value)} placeholder="也可输入绝对文件路径" /><button data-testid="open-path" onClick={() => action(async () => {
        if (!await bridge.fileInfo(openPath)) throw new Error('文件路径无效或无法读取');
        setFiles(previous => Array.from(new Set([...previous, openPath]))); setPath(openPath);
      })}>查看文件</button></div>}
      {error && <p role="alert" data-testid="operation-error">{error}</p>}
      {watchError && <p role="alert" data-testid="watch-error">自动刷新不可用：{watchError}</p>}
      {notice && <p role="status">{notice}</p>}
      {!viewer && path && <nav aria-label="文件操作">
        <button onClick={() => action(async () => { if (!await bridge.openFile(path)) throw new Error('系统打开失败'); setNotice('已交给系统默认程序打开'); })}>系统打开</button>
        <button data-testid="spawn-viewer" onClick={() => action(async () => { await bridge.spawnViewer(path); })}>只读新窗口</button>
        <button data-testid="trash-item" onClick={() => action(async () => {
          if (!await bridge.trashItem(path)) throw new Error('移到废纸篓失败');
          setFiles(previous => previous.filter(file => file !== path)); setPath(''); setNotice('已移到系统废纸篓');
        })}>移到废纸篓</button>
      </nav>}
      {preview?.kind === 'info' && <article data-testid="file-info"><h3>文件信息</h3><p>{preview.path}</p><p>{preview.size} 字节</p><p>这是二进制文件或较大文件，可使用系统默认程序查看。</p></article>}
      {preview?.kind === 'office' && <iframe title="Office 预览" data-testid="office-preview" sandbox="" srcDoc={preview.html} />}
      {preview?.kind === 'markdown' && <iframe title="Markdown 预览" data-testid="markdown-preview" sandbox="" srcDoc={markdownHtml(preview.content)} />}
      {preview && ['text', 'markdown'].includes(preview.kind) && <>
        <textarea data-testid="file-editor" aria-label="文件内容" readOnly={viewer} value={editor} onChange={event => { dirty.current = true; setEditor(event.target.value); }} />
        {!viewer && <nav aria-label="编辑操作">
          {(['cut', 'copy', 'paste', 'selectAll'] as const).map((command, i) => <button key={command} onMouseDown={e => e.preventDefault()} onClick={() => action(async () => {
            if (!await bridge.runEditCommand(command)) throw new Error('编辑命令未完成');
          })}>{['剪切', '复制', '粘贴', '全选'][i]}</button>)}
          <button data-testid="version-save" onClick={save}>保存</button>
          <button onClick={() => action(async () => {
            if (!await bridge.writeFile(path, editor)) throw new Error('写入失败');
            const snapshot = await bridge.readFileSnapshot(path);
            if (!snapshot) throw new Error('写入后未能确认文件');
            baseline.current = snapshot; dirty.current = false; setPreview(await readPreview(path, bridge)); setNotice('已直接写入');
          })}>直接覆盖写入</button>
        </nav>}
        {!viewer && preview.kind === 'markdown' && <div className="inline"><input aria-label="封面地址" value={cover} onChange={e => setCover(e.target.value)} placeholder="封面图片地址" /><button onClick={() => void changeCover(cover)}>设置封面</button><button onClick={() => void changeCover(null)}>移除封面</button></div>}
      </>}
      {!viewer && <details><summary>复制文件 / 写入附件</summary>
        <input aria-label="目标文件路径" value={destination} onChange={e => setDestination(e.target.value)} placeholder="目标绝对路径（写附件也支持 ~/）" />
        <button disabled={!path} onClick={() => action(async () => {
          if (!await bridge.copyFile(path, destination)) throw new Error('复制失败，目标未登记为可用文件');
          setFiles(previous => Array.from(new Set([...previous, destination]))); setNotice('文件已复制，源文件仍在');
        })}>复制当前文件</button>
        <textarea aria-label="附件 base64 内容" value={binaryData} onChange={e => setBinaryData(e.target.value)} placeholder="附件的 base64 字节" />
        <button onClick={() => action(async () => {
          if (!await bridge.writeFileBinary(destination, binaryData)) throw new Error('附件写入失败，未登记为有效附件');
          setNotice('附件字节已写入目标');
        })}>写入附件</button>
      </details>}
    </section>
    {viewer && <footer>只读 · 自动刷新 · 与主窗口编辑独立</footer>}
  </main>;
}
