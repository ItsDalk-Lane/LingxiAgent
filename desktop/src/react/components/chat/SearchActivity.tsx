import { useState } from 'react';
import { openInternalLink, resolveLinkTarget } from '../../utils/link-open';
import { ActivityLines, activityText as t } from './MessageActivity';
import styles from './MessageActivity.module.css';

interface SearchFile {
  path: string;
  matches?: Array<{ line?: number; text: string; context?: boolean }>;
}

function SearchFileLink({ path, basePath }: { path: string; basePath?: string }) {
  const context = { origin: 'session' as const, ...(basePath ? { baseFilePath: `${basePath.replace(/[\\/]$/, '')}/.tool-search` } : {}) };
  // 旧记录缺少搜索目录时，不猜相对文件所在位置，也不将它发送到外部浏览器。
  if (resolveLinkTarget(path, context).kind !== 'file') return <span>{path}</span>;
  return <a className={styles.link} href={path} onClick={event => { event.preventDefault(); event.stopPropagation(); void openInternalLink(path, context); }}>{path}</a>;
}

/** 搜索按总结果行数收纳，展开前不会为每个文件额外铺一整组结果。 */
export function SearchActivity({ kind, files, basePath }: { kind: string; files: SearchFile[]; basePath?: string }) {
  const [expanded, setExpanded] = useState(false);
  if (kind !== 'grep') {
    return <ActivityLines content={files.map(file => file.path).join('\n')} renderLine={line => <SearchFileLink path={line} basePath={basePath} />} />;
  }
  const rows = files.flatMap(file => (file.matches || []).map(match => ({ path: file.path, ...match })));
  const clipped = !expanded && rows.length > 8;
  const indexes = clipped ? [0, 1, 2, 3, -1, rows.length - 4, rows.length - 3, rows.length - 2, rows.length - 1] : rows.map((_, index) => index);
  return <div className={styles.lines}>{indexes.map((index, position) => {
    if (index === -1) return <button type="button" className={styles.more} key="more" onClick={() => setExpanded(true)}>{t('remaining', { n: rows.length - 8 })}</button>;
    const row = rows[index];
    const previous = indexes[position - 1];
    const heading = previous === undefined || previous === -1 || rows[previous]?.path !== row.path;
    return <div key={index}>{heading && <h4 className={styles.section}><SearchFileLink path={row.path} basePath={basePath} /></h4>}<div className={styles.line}>{typeof row.line === 'number' ? `${row.line}: ` : ''}{row.text}</div></div>;
  })}</div>;
}
