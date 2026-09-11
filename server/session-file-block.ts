import path from "node:path";

/** 实时交付和历史重建共用同一份文件证据，当前位置不替代内容版本。 */
export function sessionFileToContentBlock(file: any, extra: any = undefined) {
  if (!file || typeof file !== "object") return null;
  const filePath = file.filePath || file.realPath || null;
  if (!filePath) return null;
  const fileId = file.fileId || file.id || null;
  const label = file.label || file.displayName || file.filename || path.basename(filePath);
  const ext = file.ext ?? path.extname(filePath || label).toLowerCase().replace(/^\./, "");
  return {
    type: "file",
    ...(extra || {}),
    ...(fileId ? { fileId } : {}),
    filePath,
    label,
    ext,
    ...sessionFileFields(file),
  };
}

export function sessionFileFields(file: any) {
  if (!file || typeof file !== "object") return {};
  const fileId = file.fileId || file.id || null;
  return {
    ...(fileId ? { fileId } : {}),
    ...(file.filePath ? { filePath: file.filePath } : {}),
    ...(file.label ? { label: file.label } : {}),
    ...(file.ext !== undefined ? { ext: file.ext } : {}),
    ...(file.mime ? { mime: file.mime } : {}),
    ...(file.kind ? { kind: file.kind } : {}),
    ...(file.storageKind ? { storageKind: file.storageKind } : {}),
    ...(file.presentation ? { presentation: file.presentation } : {}),
    ...(file.listed !== undefined ? { listed: file.listed !== false } : {}),
    ...(file.status ? { status: file.status } : {}),
    ...(file.missingAt !== undefined ? { missingAt: file.missingAt } : {}),
    ...(file.mtimeMs !== undefined ? { mtimeMs: file.mtimeMs } : {}),
    ...(file.size !== undefined ? { size: file.size } : {}),
    ...(file.version ? { version: file.version } : {}),
    ...(file.waveform ? { waveform: file.waveform } : {}),
    ...(file.resource ? { resource: file.resource } : {}),
  };
}
