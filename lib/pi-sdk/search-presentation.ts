import type { ToolSearchPresentation } from "../../shared/tool-presentation.ts";

// 和工具正文的 50 KB 上限同量级，防止用户放大搜索 limit 后附带无界元数据。
const SEARCH_PRESENTATION_MAX_BYTES = 50 * 1024;
const SEARCH_PRESENTATION_MAX_ROWS = 1000;

export function boundedSearchPresentation(value: ToolSearchPresentation): ToolSearchPresentation {
  const files: NonNullable<ToolSearchPresentation["files"]> = [];
  let bytes = 0;
  let rows = 0;
  let truncated = value.truncated === true;
  for (const file of value.files ?? []) {
    const pathBytes = Buffer.byteLength(file.path, "utf-8") + 32;
    if (bytes + pathBytes > SEARCH_PRESENTATION_MAX_BYTES || rows >= SEARCH_PRESENTATION_MAX_ROWS) {
      truncated = true;
      break;
    }
    bytes += pathBytes;
    rows++;
    const retained = { path: file.path, ...(file.matches ? { matches: [] as NonNullable<typeof file.matches> } : {}) };
    files.push(retained);
    for (const match of file.matches ?? []) {
      const matchBytes = Buffer.byteLength(match.text, "utf-8") + 48;
      if (bytes + matchBytes > SEARCH_PRESENTATION_MAX_BYTES || rows >= SEARCH_PRESENTATION_MAX_ROWS) {
        truncated = true;
        break;
      }
      bytes += matchBytes;
      rows++;
      retained.matches?.push(match);
    }
  }
  return { ...value, files, truncated };
}
