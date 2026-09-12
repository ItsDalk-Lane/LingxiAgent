import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import type { FileRollbackReport } from '../../stores/chat-slice';
import styles from './SessionRollback.module.css';

const t = (key: string, vars?: Record<string, string | number>): string => {
  const translate = window.t;
  return translate ? translate(key, vars) : key;
};

/**
 * 「回退时撤销文件改动」完成后的逐文件报告横幅。
 *
 * 数据源是 chat store 的 fileRollbackReportsByPath：HTTP 响应与 ws
 * session_branch_reset 事件都写同一个键，所以无论从哪个窗口触发都能看到。
 */
export function FileRollbackReportBanner({ sessionPath }: { sessionPath: string }) {
  const report: FileRollbackReport | undefined = useStore(
    (state: any) => sessionScopedValue(state, state.fileRollbackReportsByPath, sessionPath),
  );
  const clearFileRollbackReport = useStore((state: any) => state.clearFileRollbackReport);
  if (!report) return null;

  const failures = Array.isArray(report.failures) ? report.failures.length : 0;
  const files = Array.isArray(report.files) ? report.files : [];
  const succeeded = Math.max(0, files.length - failures);
  const summary = failures > 0
    ? t('chat.fileRollback.reportPartial', { failed: failures, count: succeeded })
    : t('chat.fileRollback.reportAllOk', { count: succeeded });

  return (
    <div className={styles.reportBanner} data-testid="file-rollback-report" role="status">
      <div className={styles.reportHead}>
        <strong>{t('chat.fileRollback.reportTitle')}</strong>
        <button
          type="button"
          className={styles.reportDismiss}
          onClick={() => clearFileRollbackReport?.(sessionPath)}
        >
          {t('chat.fileRollback.dismiss')}
        </button>
      </div>
      <div className={styles.reportSummary} data-report-ok={report.ok ? 'true' : 'false'}>
        {summary}
      </div>
      {report.degraded && (
        <div className={styles.reportWarning} data-testid="file-rollback-report-degraded">
          {t('chat.fileRollback.reportDegraded')}
        </div>
      )}
      {files.length > 0 && (
        <ul className={styles.reportFiles}>
          {files.map((file: any, index: number) => (
            <li
              key={`${file.path}:${index}`}
              className={file.ok ? styles.reportFileOk : styles.reportFileFail}
              data-file-ok={file.ok ? 'true' : 'false'}
            >
              <span className={styles.reportFilePath}>{file.path}</span>
              <span className={styles.reportFileAction}>{file.action}</span>
              {file.source === 'backup' && (
                <span className={styles.reportFileSource}>{t('chat.fileRollback.reportSourceBackup')}</span>
              )}
              {!file.ok && file.reason && (
                <span className={styles.reportFileReason}>{file.reason}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
