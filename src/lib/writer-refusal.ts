// Closed vocabulary for the in-renderer PDF writer (MuPDF.js/WASM + pinned font).
//
// Why this module exists: the writer can fail in several different places, and
// every raw engine failure is untrusted text that must never be displayed. The
// previous revision threw bare `Error` objects with free-form English messages,
// so the renderer's safe formatter had no approved code to report and every
// failure collapsed into "原因未能安全识别" — the user could not tell whether the
// resources, the WASM engine, the font, the annotation serialization or the
// appended revision was the actual first failing step.
//
// Every refusal now carries exactly one approved code and the step it happened
// in. The underlying engine error is retained only as a local `cause` for
// debugging; it is never displayed and never leaves the renderer.
export const writerSteps = {
  resources: '资源校验',
  engine: 'MuPDF.js 引擎初始化',
  font: '固定字体加载',
  serialize: '批注序列化',
  appearance: '批注外观写入',
  revision: '生成追加修订',
} as const;
export type WriterStep = keyof typeof writerSteps;

/** Only these fixed strings may ever be shown or logged for a writer failure. */
export const writerRefusalMessages = {
  writer_font_fetch_failed: '固定 Unicode 字体未能取回，未启动写入层；原 PDF 未修改。',
  writer_engine_unavailable: 'MuPDF.js 引擎初始化失败，未启动写入层；原 PDF 未修改。',
  writer_font_load_failed: '固定 Unicode 字体无法载入引擎，未写入副本；原 PDF 未修改。',
  writer_renderer_busy: '验证前请等待页面和批注恢复完成。',
  writer_document_changed: '文档已切换，验证副本未保存。',
  writer_baseline_invalid: '无法核对原文件与本次批注的差异，请保留草稿并重新打开 PDF；原 PDF 未修改。',
  writer_no_annotation: '本次没有可写入的批注；原 PDF 未修改。',
  writer_glyph_missing: '固定字体缺少所需字形，未写入副本；原 PDF 未修改。',
  writer_subset_failed: '字体子集化失败，未写入副本；原 PDF 未修改。',
  writer_encode_failed: '批注文本无法编码，未写入副本；原 PDF 未修改。',
  writer_geometry_refused: '批注几何无效，未写入副本；原 PDF 未修改。',
  writer_text_overflow: '文字超出批注框，请放大文字框或减小字号后再保存；原 PDF 未修改。',
  writer_revision_missing: '写入层未产生可校验的追加修订，未导出；原 PDF 未修改。',
  writer_internal_failed: '写入层在该步骤内未完成（内部未分类失败）；原 PDF 未修改。',
} as const;
export type WriterRefusalCode = keyof typeof writerRefusalMessages;

/**
 * A writer failure with an approved code and step. Own data properties only, so
 * the renderer's formatter can read them without touching getters or unknown
 * rejection objects, and the engine text stays in `cause`.
 */
export class WriterRefusal extends Error {
  readonly code: WriterRefusalCode;
  readonly writerStep: WriterStep;
  /** Bounded HTTP status of a failed resource request, when there was one. */
  readonly httpStatus?: number;
  constructor(code: WriterRefusalCode, writerStep: WriterStep, cause?: unknown, httpStatus?: number) {
    super(writerRefusalMessages[code], cause === undefined ? undefined : { cause });
    this.name = 'WriterRefusal';
    this.code = code;
    this.writerStep = writerStep;
    this.httpStatus = Number.isInteger(httpStatus) && (httpStatus as number) >= 0 && (httpStatus as number) <= 599 ? httpStatus : undefined;
    for (const key of ['code', 'writerStep', 'httpStatus']) Object.defineProperty(this, key, { writable: false, configurable: false });
  }
}

/**
 * Run one writer step. An approved refusal keeps its own code and step; any
 * unexpected engine failure is classified at the step that was running, so a
 * failure can never be reported without a code again.
 */
export function writerStep<T>(step: WriterStep, run: () => T): T {
  try {
    return run();
  } catch (error) {
    if (error instanceof WriterRefusal) throw error;
    throw new WriterRefusal('writer_internal_failed', step, error);
  }
}
