import type { QuotedSelection } from '../../stores/input-slice';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

/**
 * 引用新鲜度重读（阶段一·2）。
 *
 * 注意落点：这个函数曾放在 utils/quoted-selection.ts，但那边被
 * utils/message-parser.ts 常量引用，处在 stores 的模块求值图里——
 * 一旦 value-import use-hana-fetch（其首行就建 store）就会织出
 * stores/index → chat-slice（求值中）→ … → use-hana-fetch → stores/index
 * 的环，createChatSlice 在环里还是 undefined，chat-slice 及一批依赖
 * stores 的组件测试全体炸掉。所以重读逻辑必须住在组件层（本文件）。
 */

/** 引用新鲜度重读的大小上限：文件超过 4MB 就信任捕获时刻的文本。 */
const QUOTE_REREAD_MAX_BYTES = 4 * 1024 * 1024;

/**
 * 发送前按 {sourceFilePath, lineStart, lineEnd} 现读一次文件切片：
 * 引用之后文件可能被改过，模型看到的应是「该处当前内容」而不是捕获时刻的
 * 旧文本。任何一步失败（文件删了/读不了/超限）都回落为捕获文本。
 */
export async function refreshQuotedSelectionFromDisk(sel: QuotedSelection): Promise<QuotedSelection> {
  if (!sel.sourceFilePath || sel.lineStart == null || sel.lineEnd == null) return sel;
  try {
    const res = await lingxiFetch(`/api/fs/read?path=${encodeURIComponent(sel.sourceFilePath)}`);
    if (!res.ok) return sel;
    const buf = await res.arrayBuffer();
    if (buf.byteLength > QUOTE_REREAD_MAX_BYTES) return sel;
    const fresh = new TextDecoder('utf-8').decode(buf);
    const lines = fresh.split('\n');
    if (sel.lineStart < 1 || sel.lineStart > lines.length) return sel;
    const sliced = lines.slice(sel.lineStart - 1, sel.lineEnd).join('\n').trim();
    if (!sliced || sliced === sel.text) return sel;
    return { ...sel, text: sliced, charCount: sliced.length };
  } catch {
    return sel;
  }
}
