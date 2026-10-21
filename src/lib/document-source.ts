// Tauri can provide the same read-only bytes after backend authorization.
// Browser File objects carry no persistent document identity or original path.
export interface PdfSource {
  readonly name: string;
  readonly size: number;
  // Opaque, window-scoped desktop permission. Browser files never have one.
  readonly grantId?: string;
  read(signal: AbortSignal): Promise<Uint8Array>;
  // Opt-in M3 debug capability. Always creates a new bounded test copy.
  testCopy?(bytes: Uint8Array): Promise<PdfSource>;
  exportCopy?(bytes: Uint8Array, signal: AbortSignal): Promise<PdfSource | null>;
  saveOriginal?(bytes: Uint8Array, signal: AbortSignal, bindingId?:string): Promise<OriginalSaved>;
  release?(): void | Promise<void>;
}
export interface OriginalSaved {
  source:PdfSource|null; hash:string; binding:import('./local-data').DocumentBinding|null;
  historyPending:boolean; reopenRequired:boolean;
  cleanupPending?:boolean;
}

export const MAX_INPUT_BYTES = 64 * 1024 * 1024; // Development guard, not final product capacity.

export class SourceError extends Error {
  constructor(message: string) { super(message); this.name = 'SourceError'; }
}

export function browserFileSource(file: File): PdfSource {
  return {
    name: file.name, size: file.size,
    async read(signal) {
      signal.throwIfAborted();
      const bytes = new Uint8Array(await file.arrayBuffer());
      signal.throwIfAborted();
      return bytes;
    },
  };
}

export function validateSource(source: PdfSource) {
  if (!/\.pdf$/i.test(source.name)) throw new SourceError('请选择 PDF 文件。');
  if (!Number.isSafeInteger(source.size) || source.size < 1) throw new SourceError('文件为空或大小无效。');
  if (source.size > MAX_INPUT_BYTES) throw new SourceError('本阶段暂支持不超过 64 MB 的 PDF，请先使用较小文件。');
}

export function readableError(error: unknown): string {
  if (error instanceof SourceError) return error.message;
  const name = error instanceof Error ? error.name : '';
  if (name === 'PasswordException') return '这份 PDF 需要密码；密码输入将在后续阶段提供，请先打开未加密文件。';
  if (name === 'InvalidPDFException') return '无法读取这份 PDF，文件可能已损坏。可以重新选择其他文件。';
  return '暂时无法读取这份 PDF，请重新选择文件。原文件未被修改。';
}
