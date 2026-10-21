import { invoke, isTauri } from '@tauri-apps/api/core';
import { SourceError, MAX_INPUT_BYTES, type PdfSource } from './document-source.ts';
import { WriterRefusal, writerRefusalMessages, writerSteps, type WriterStep } from './writer-refusal.ts';
import { appendAnnotationRevision, loadMupdf } from './mupdf-writer.ts';
import { loadFreeTextFont, prepareFreeTextFont } from './freetext-layout.ts';
import {validateOriginalOutput} from './original-output.ts';
import {saveHeaders} from './save-transport.ts';

export interface DesktopDescriptor { grantId: string; name: string; size: number; }
export type DesktopBridge = <T>(command: string, args?: Record<string, unknown>|Uint8Array, options?:{headers:Record<string,string>}) => Promise<T>;
export const isDesktop = () => typeof window !== 'undefined' && isTauri();
const messages = {
  ...writerRefusalMessages,
  unauthorized: '文件授权已失效，请重新打开。', cancelled: '读取已取消。',
  busy: '已有读取任务或打开的文件过多，请稍后重试。',
  file_unavailable: '无法读取文件，可能被其他程序占用，请重新选择。',
  invalid_pdf: '请选择非空、本地且不超过 64 MB 的 PDF 文件。', file_changed: '文件已发生变化，请关闭后重新打开。',
  test_writer_refused: '验证副本未完成：仅支持项目测试目录、单页和至多一个黑色文本框；原 PDF 未修改。',
  writer_candidate_disabled:'此版本未启用候选 Writer；原 PDF 未修改。',
  writer_pdf_budget:'当前另存为支持不超过 16 MB 的 PDF；原文件未修改。',
  writer_synthetic_only:'候选 Writer 仅接受已批准的合成样例，不处理个人 PDF。',
  writer_isolation_blocked:'Writer 隔离检查未通过，请在 Sandbox 中完成隔离自检；原 PDF 未修改。',
  writer_resource_mismatch:'Writer 资源清单或文件校验失败，未启动。',
  writer_profile_rejected:'Writer 专属隔离配置身份或权限异常，保留现场并拒绝认领。',
  writer_profile_create_failed:'Writer 隔离配置创建或资源授权校验未完成，已有配置保留。',
  writer_startup_2:'受限 Writer 启动失败（Windows 2），未改用普通权限。',
  writer_probe_failed:'受限 Writer 权限探针未通过，未启用导出。',
  writer_standards_blocked:'Writer 输出未通过 PDF 标准结构检查，未导出；原 PDF 和恢复稿保留。',
  writer_conversion_refused:'此 PDF 的保护格式、批注或结构暂不支持安全转换；原 PDF 未修改。',
  writer_source_changed:'原文件在打开后被修改、占用或被替换；未写入。请重新打开该文件后再保存。',
  writer_original_disabled:'原件保存尚未通过安全验收，当前保持禁用；请使用“另存为批注副本”。原 PDF 未修改。',
  writer_original_unsupported:'这份 PDF 不符合当前安全增量保存范围，请另存为批注副本。原件未修改。',
  writer_original_geometry_changed:'保存检查发现页面尺寸或旋转不一致，未写入原件。',
  writer_original_annotations_changed:'保存检查发现批注内容或位置不一致，未写入原件。',
  writer_original_text_changed:'保存检查发现原文内容不一致，未写入原件。',
  writer_original_reader_failed:'保存检查未能重新读取生成的 PDF，未写入原件。',
  writer_original_structure_invalid:'保存检查未通过 PDF 增量结构核验，未写入原件。',
  writer_original_reopen_required:'未取得原件写入保护或检测到外部修改，未写入。请保留编辑，另存副本或关闭后重新打开。',
  writer_original_io_failed:'原件保存未完成，备份和新版材料已保留。请重新打开文件检查恢复，不要重复保存。',
  writer_original_interrupted:'保存中断或最终校验未完成。请重新打开文件，从恢复提示另存新版或旧版；原件未被自动回滚。',
  writer_original_materials_invalid:'保存恢复材料无法确认，未自动修改原件。请保留文件与恢复目录。',
  writer_recovery_required:'发现未完成保存，请先检查恢复副本。原件未被覆盖。',
  writer_history_pending:'PDF 已保存，历史关联尚未完成。旧历史和草稿保留，请重新打开 PDF，不要重复保存。',
  writer_response_invalid:'保存命令返回的结果不符合约定，保存结果尚未确认。请保留当前窗口和恢复稿，检查目标文件后再操作。',
  writer_target_exists:'你在系统对话框里选择了已存在的文件；本应用不会覆盖任何文件，批注仍在窗口中。请换一个文件名再另存。',
  writer_target_access_denied:'目标目录不允许创建副本；请选一个可写目录。原 PDF 未修改。',
  writer_target_locked:'目标文件或目录被占用，未创建副本；请换一个新文件名或稍后重试。',
  writer_target_disk_full:'目标磁盘空间不足，未完成副本；原 PDF 和恢复稿保留。',
  writer_export_failed:'副本未完成，新文件可能不完整；原 PDF 和恢复稿保留。',
  writer_export_cancelled:'导出已取消，可能产生独立副本；原 PDF 未修改。',
} as const;
type DesktopErrorCode = keyof typeof messages;
const fileFallback = '桌面文件读取暂时不可用，请重试。原文件未被修改。';
export type WriterStage = 'check' | 'serialize' | 'begin' | 'finish' | 'reopen' | 'original';
const writerStages: Record<WriterStage, string> = {
  check: 'Writer 隔离检查', serialize: '生成 PDF 批注', begin: '准备另存为',
  finish: '写入批注副本', reopen: '重新打开批注副本',
  original:'保存 PDF',
};
const sandboxStages = {
  identity: '隔离身份', 'profile-query': '查询隔离配置', 'profile-create': '创建隔离配置',
  'profile-path': '隔离目录定位', 'profile-root': '隔离主目录权限', 'profile-ac': '隔离工作目录权限',
  'runtime-acl': 'Writer 资源权限', marker: '隔离配置标记', setup: '隔离配置准备',
  'worker-pipes': '进程通信初始化', 'worker-policy': '进程隔离策略', 'worker-job-limits': '进程资源限制配置',
  'worker-job-assignment': '进程资源限制绑定', 'worker-start': '启动受限进程', 'worker-token': '受限进程身份',
  'worker-resume': '运行受限进程', 'worker-exit': '受限进程异常退出',
  'worker-token-open': '读取受限进程身份', 'worker-token-app': '核对隔离进程类型',
  'worker-token-integrity': '核对低权限等级', 'worker-token-capabilities': '核对附加权限',
  'worker-token-package': '核对所属应用', 'worker-token-access': '核对实际隔离授权',
  'probe-appcontainer-control': '普通隔离进程对照', 'probe-lpac-control': '严格隔离进程对照',
  'entry-start': '运行库入口启动', 'entry-path': '定位随包运行库', 'entry-args': '核对固定处理任务',
  'entry-crt-primary': '载入随包 C 运行库', 'entry-crt-secondary': '载入随包 C++ 运行库',
  'entry-python-load': '载入 Python 核心及其依赖', 'entry-python-symbol': '查找 Python 固定入口',
  'entry-python-main': '初始化 Python', 'entry-python-return': 'Python 任务返回错误',
  'probe-runtime': '受限进程自检', 'probe-resources': '自检资源校验', 'probe-sentinel': '创建私有测试文件',
  'probe-cleanup': '回收私有测试文件', 'probe-permissions': '权限自检响应',
  'probe-private-read': '禁止读取私有文件', 'probe-runtime-write': '禁止写入运行资源',
  'probe-network': '网络隔离证明', 'probe-child': '禁止启动子进程', 'probe-memory': '内存限制验证',
  'probe-timeout': '超时终止验证', 'probe-cancel': '取消终止验证', 'probe-output': '输出上限验证',
} as const;
const sandboxReasons = {
  'access-denied': 'Windows 拒绝访问', 'not-found': '所需资源不存在',
  'linked-resource': '资源是链接或类型不符', owner: '资源所有者不符合要求',
  'dacl-missing': '缺少访问权限表', 'dacl-unprotected': '资源仍继承外部权限',
  'ace-type': '出现不支持的权限项类型', 'ace-sid': '权限身份格式无效',
  'access-mask': '权限范围不符合要求', 'package-write': 'Writer 获得了不允许的写权限',
  principal: '出现未授权的访问身份', 'incomplete-dacl': '所需权限不完整',
  'path-mapping': '隔离目录位置不符合要求', 'path-canonical': '目录实际位置不符合要求',
  'unmarked-profile': '隔离配置已存在，但缺少本应用的有效标记',
  'marker-identity': '隔离配置标记与当前身份不一致', 'concurrent-profile': '其他进程同时创建了隔离配置',
  'os-error': 'Windows 操作未完成', 'environment-missing': 'Windows 隔离启动所需环境信息缺失', unknown: '原因尚未能确定',
  'child-exited': '进程提前异常退出', cancelled: '检查已取消', timeout: '进程超时', busy: '已有处理任务',
  'pipe-failure': '进程通信或输出上限失败', 'denial-unproven': '未能证明所需操作已被禁止',
  'protocol-invalid': '自检响应格式无效', 'limit-unproven': '未能证明限制生效',
  'token-invalid': '进程隔离身份不符合要求', 'termination-unresolved': '进程终止状态无法确认',
  'isolation-mismatch': '实际授权结果未满足完整隔离对照',
  'entry-initialization': '运行库初始化提前终止',
  'python-module': 'Python 缺少模块', 'python-import': 'Python 无法载入依赖', 'python-access': 'Python 遇到访问拒绝',
  'python-file': 'Python 所需文件不存在', 'python-memory': 'Python 内存分配失败',
} as const;
type SandboxStage = keyof typeof sandboxStages;
type SandboxReason = keyof typeof sandboxReasons;
interface SandboxDiagnostic { sandboxStage?: SandboxStage; sandboxReason?: SandboxReason; windowsError?: number; workerExit?: number; }
function hasSandboxDiagnostic(code: DesktopErrorCode | undefined): boolean {
  return code === 'writer_profile_rejected' || code === 'writer_profile_create_failed' || code === 'writer_probe_failed' || code === 'writer_startup_2';
}
function ownData(error: unknown, key: string): unknown {
  if (!error || typeof error !== 'object') return;
  try {
    if (Array.isArray(error)) return;
    const field = Object.getOwnPropertyDescriptor(error, key);
    return field && 'value' in field ? field.value : undefined;
  } catch { /* Unknown proxies must not prevent safe, fixed diagnostic text. */ }
}
function copyDescriptor(value: unknown): DesktopDescriptor | null {
  const grantId=ownData(value,'grantId'),name=ownData(value,'name'),size=ownData(value,'size');
  return typeof grantId==='string'&&/^[a-f0-9-]{36}$/.test(grantId)&&typeof name==='string'&&/\.pdf$/i.test(name)
    &&typeof size==='number'&&Number.isSafeInteger(size)&&size>=1&&size<=MAX_INPUT_BYTES
    ? {grantId,name,size} : null;
}
function sandboxDiagnostic(error: unknown): SandboxDiagnostic {
  const stage = ownData(error, 'sandboxStage');
  const reason = ownData(error, 'sandboxReason');
  const windowsError = ownData(error, 'windowsError');
  const workerExit = ownData(error, 'workerExit');
  return {
    sandboxStage: typeof stage === 'string' && Object.hasOwn(sandboxStages, stage) ? stage as SandboxStage : undefined,
    sandboxReason: typeof reason === 'string' && Object.hasOwn(sandboxReasons, reason) ? reason as SandboxReason : undefined,
    windowsError: typeof windowsError === 'number' && Number.isInteger(windowsError) && windowsError >= 0 && windowsError <= 0xffffffff
      ? windowsError : undefined,
    workerExit: typeof workerExit === 'number' && Number.isInteger(workerExit) && workerExit >= 0 && workerExit <= 0xffffffff
      ? workerExit : undefined,
  };
}
function sandboxDetail(error: unknown): string {
  const diagnostic = sandboxDiagnostic(error);
  const details: string[] = [];
  if (diagnostic.sandboxStage) details.push(`检查位置：${sandboxStages[diagnostic.sandboxStage]}`);
  if (diagnostic.sandboxReason) details.push(`原因：${sandboxReasons[diagnostic.sandboxReason]}`);
  if (diagnostic.windowsError !== undefined) details.push(`Windows 错误：${diagnostic.windowsError}（0x${diagnostic.windowsError.toString(16).padStart(8, '0')}）`);
  if (diagnostic.workerExit !== undefined) details.push(`进程退出码：${diagnostic.workerExit}（0x${diagnostic.workerExit.toString(16).padStart(8, '0')}）`);
  return details.length ? ` ${details.join('；')}。` : '';
}
function safeDesktopCode(error: unknown): DesktopErrorCode | undefined {
  if (!error || typeof error !== 'object') return;
  try {
    if (Array.isArray(error)) return;
    // Read data properties only: do not invoke a getter/toString or serialize an
    // unknown rejection object. Backend message/stack/path are never displayed.
    const code = Object.getOwnPropertyDescriptor(error, 'code');
    if (code) return 'value' in code && typeof code.value === 'string' && Object.hasOwn(messages, code.value)
      ? code.value as DesktopErrorCode : undefined;
    // SourceError is retained by existing PdfSource callers. Recover only an
    // exact, already-public static message when no native code is available.
    const message = Object.getOwnPropertyDescriptor(error, 'message');
    if (message && 'value' in message && typeof message.value === 'string') {
      return (Object.keys(messages) as DesktopErrorCode[]).find(code => messages[code] === message.value);
    }
  } catch { /* Unknown proxies/rejections are untrusted diagnostic data. */ }
}
export function desktopError(error: unknown): string {
  const code = safeDesktopCode(error);
  return code ? messages[code] : fileFallback;
}
function safeWriterStage(error: unknown): WriterStage | undefined {
  if (!error || typeof error !== 'object') return;
  try {
    if (Array.isArray(error)) return;
    const stage = Object.getOwnPropertyDescriptor(error, 'writerStage');
    return stage && 'value' in stage && typeof stage.value === 'string' && Object.hasOwn(writerStages, stage.value)
      ? stage.value as WriterStage : undefined;
  } catch { /* Never inspect unknown getter/proxy behavior for a diagnostic. */ }
}
/** The writer step an approved refusal happened in, from own data only. */
function safeWriterStep(error: unknown): WriterStep | undefined {
  if (!error || typeof error !== 'object') return;
  try {
    if (Array.isArray(error)) return;
    const step = Object.getOwnPropertyDescriptor(error, 'writerStep');
    return step && 'value' in step && typeof step.value === 'string' && Object.hasOwn(writerSteps, step.value)
      ? step.value as WriterStep : undefined;
  } catch { /* Never inspect unknown getter/proxy behavior for a diagnostic. */ }
}
/** Bounded HTTP status of the failed resource request, from own data only. */
function safeWriterHttpStatus(error: unknown): number | undefined {
  if (!error || typeof error !== 'object') return;
  try {
    if (Array.isArray(error)) return;
    const status = Object.getOwnPropertyDescriptor(error, 'httpStatus');
    return status && 'value' in status && typeof status.value === 'number' && Number.isInteger(status.value)
      && status.value >= 0 && status.value <= 599 ? status.value : undefined;
  } catch { /* Never inspect unknown getter/proxy behavior for a diagnostic. */ }
}
function writerStepDetail(error: unknown): string {
  const details: string[] = [];
  const step = safeWriterStep(error);
  if (step) details.push(`步骤：${writerSteps[step]}`);
  const status = safeWriterHttpStatus(error);
  if (status !== undefined) details.push(`HTTP 状态：${status}`);
  return details.length ? ` ${details.join('；')}。` : '';
}
// A single safe formatter for native AppError objects and frontend failures.
// Preserve the operation, the public error code and, for the in-renderer writer,
// the exact step that failed — without echoing arbitrary text.
function safeStructureRule(error:unknown):string|undefined {
  if(safeDesktopCode(error)!=='writer_original_structure_invalid')return;
  const value=ownData(error,'structureRule')??ownData(error,'message');
  // Emit only the fixed three-character rule number, never input-derived text.
  if(typeof value==='string'&&/^(?:I(?:0[0-9]|1[0-3])|T(?:0[1-9]|1[0-2])|S(?:0[1-9]|1[0-3]))(?:_[a-z_]+)?$/.test(value))return value.slice(0,3);
}
export function writerError(error: unknown, stage: WriterStage = 'finish'): string {
  const actualStage = safeWriterStage(error) ?? stage;
  const label = Object.hasOwn(writerStages, actualStage) ? writerStages[actualStage] : writerStages.finish;
  const code = safeDesktopCode(error);
  const rule=safeStructureRule(error);
  const detail = `${hasSandboxDiagnostic(code) ? sandboxDetail(error) : ''}${writerStepDetail(error)}${rule?` 检查编号：${rule}。`:''}`;
  // An unknown rejection does not establish whether the backend ran: transport,
  // post-commit response handling and renderer code can all fail without a code.
  // Never infer that the original was unchanged from the missing code alone.
  return code ? `${label}未完成（${code}）：${messages[code]}${detail}`
    : `${label}未完成（未取得错误码，无法确认失败阶段与保存结果）。请保留当前窗口和恢复稿，先检查目标文件，勿重复覆盖。`;
}
export class DesktopSourceError extends SourceError {
  recoverySource?:PdfSource;
  readonly code: DesktopErrorCode | undefined;
  readonly writerStage: WriterStage;
  readonly sandboxStage?: SandboxStage;
  readonly sandboxReason?: SandboxReason;
  readonly windowsError?: number;
  readonly workerExit?: number;
  readonly writerStep?: WriterStep;
  readonly httpStatus?: number;
  readonly structureRule?: string;
  constructor(error: unknown, stage: WriterStage) {
    super(desktopError(error)); this.code = safeDesktopCode(error); this.writerStage = stage;
    this.structureRule=safeStructureRule(error);
    Object.defineProperty(this, 'writerStage', { writable: false, configurable: false });
    if (hasSandboxDiagnostic(this.code)) {
      const diagnostic = sandboxDiagnostic(error);
      this.sandboxStage = diagnostic.sandboxStage; this.sandboxReason = diagnostic.sandboxReason; this.windowsError = diagnostic.windowsError; this.workerExit = diagnostic.workerExit;
      for (const key of ['sandboxStage', 'sandboxReason', 'windowsError', 'workerExit']) Object.defineProperty(this, key, { writable: false, configurable: false });
    }
    // The writer step must survive the export/read wrapping, otherwise the
    // reported failure would lose the step it actually happened in.
    this.writerStep = safeWriterStep(error);
    if (this.writerStep) Object.defineProperty(this, 'writerStep', { writable: false, configurable: false });
    this.httpStatus = safeWriterHttpStatus(error);
    if (this.httpStatus !== undefined) Object.defineProperty(this, 'httpStatus', { writable: false, configurable: false });
  }
}
/** Renderer-side byte layer: candidate bytes in, one appended revision out. */
export type CandidateWriter = (candidate: Uint8Array, signal: AbortSignal, original: Uint8Array) => Promise<Uint8Array>;

/** Default writer: MuPDF.js/WASM, pinned font, no file system access. */
export const mupdfCandidateWriter: CandidateWriter = async (candidate, signal, original) => {
  // The writer module is part of the main bundle on purpose. A lazily imported
  // chunk would add a module-loading path that no packaged build had ever
  // exercised, and its failure could only be reported as an unnamed resource
  // problem. The engine (several megabytes) and the pinned font stay lazy, and
  // each of those failures names its own step.
  const [mupdf, font] = await Promise.all([
    loadMupdf().catch((cause) => { throw new WriterRefusal('writer_engine_unavailable', 'engine', cause); }),
    loadFreeTextFont().catch((cause) => { throw cause instanceof WriterRefusal ? cause : new WriterRefusal('writer_font_fetch_failed', 'resources', cause); }),
    prepareFreeTextFont(),
  ]);
  signal.throwIfAborted();
  const result = appendAnnotationRevision(mupdf, candidate, font, original);
  signal.throwIfAborted();
  return result.bytes;
};

export function desktopSource(descriptor: DesktopDescriptor, bridge: DesktopBridge = invoke, writeCandidate: CandidateWriter = mupdfCandidateWriter, validateOriginal=validateOriginalOutput): PdfSource {
  if (!/^[a-f0-9-]{36}$/.test(descriptor.grantId) || !/\.pdf$/i.test(descriptor.name) ||
      !Number.isSafeInteger(descriptor.size) || descriptor.size < 1 || descriptor.size > MAX_INPUT_BYTES) throw new SourceError('文件授权信息无效，请重新选择。');
  let released = false;
  // PDF.js serialization retains the full authorized original as its prefix.
  // Its exact length is already attached to this grant, refreshed after save
  // and reopen. No extra read, retained PDF copy, editor state or IPC is needed.
  // Native prepare() still verifies every prefix byte against its own snapshot.
  const originalPrefix = (candidate: Uint8Array) => {
    if (candidate.length < descriptor.size) throw new WriterRefusal('writer_baseline_invalid', 'serialize');
    return candidate.subarray(0, descriptor.size);
  };
  // Save As alone returns a new file grant. Original-save results must never be
  // interpreted as descriptors: the earlier native command returned a string.
  const produceCopy = async (bytes: Uint8Array, signal: AbortSignal) => {
    signal.throwIfAborted();if(released)throw new SourceError(messages.unauthorized);if(bytes.byteLength>16*1024*1024)throw new SourceError(messages.writer_pdf_budget);
    let task:string|undefined,stage:WriterStage='begin';
    const cancel=()=>{if(task)void bridge('cancel_unicode_export',{taskId:task}).catch(()=>{});};
    try{
      task=await bridge<string>('begin_unicode_export',{grantId:descriptor.grantId});signal.addEventListener('abort',cancel,{once:true});
      if(signal.aborted||released){cancel();signal.throwIfAborted();throw new SourceError(messages.unauthorized);}
      stage='serialize';const output=await writeCandidate(bytes,signal,originalPrefix(bytes));
      if(signal.aborted||released){cancel();signal.throwIfAborted();throw new SourceError(messages.unauthorized);}
      if(!(output instanceof Uint8Array)||output.byteLength<=bytes.byteLength||output.byteLength>20*1024*1024){cancel();throw new SourceError(messages.writer_standards_blocked);}
      stage='finish';const response=await bridge<unknown>('finish_unicode_export',output,saveHeaders(task,bytes.byteLength));
      const result = response === null ? null : copyDescriptor(response);
      if(response !== null && !result){
        // Only a well-formed opaque grant may be released; never trust a path
        // or evaluate a getter from an invalid response.
        const grantId=ownData(response,'grantId');
        if(typeof grantId==='string'&&/^[a-f0-9-]{36}$/.test(grantId))await bridge('release_pdf',{grantId}).catch(()=>{});
        throw new SourceError(messages.writer_response_invalid);
      }
      if(signal.aborted||released){if(result)await bridge('release_pdf',{grantId:result.grantId}).catch(()=>{});signal.throwIfAborted();throw new SourceError(messages.unauthorized);}
      return result?desktopSource(result,bridge,writeCandidate):null;
    }catch(error){signal.throwIfAborted();throw new DesktopSourceError(error,stage);}
    finally{signal.removeEventListener('abort',cancel);cancel();}
  };
  return {
    name: descriptor.name, size: descriptor.size, grantId: descriptor.grantId,
    exportCopy(bytes,signal) {
      return produceCopy(bytes,signal);
    },
    async saveOriginal(bytes,signal,bindingId) {
      const started=performance.now();
      for(const key of ['writer','validation','ipc','total','native-validationMs','native-backupMs','native-commitMs','native-historyMs','native-housekeepingMs','native-decodeMs','native-queueMs','native-commandMs','native-commitTotalMs'])performance.clearMeasures('pdf-save:'+key);
      signal.throwIfAborted();if(released)throw new SourceError(messages.unauthorized);
      if(bytes.byteLength>16*1024*1024)throw new SourceError(messages.writer_pdf_budget);
      let task:string|undefined;const cancel=()=>{if(task)void bridge('cancel_unicode_export',{taskId:task}).catch(()=>{});};
      try {
        task=await bridge<string>('begin_unicode_export',{grantId:descriptor.grantId});
        signal.addEventListener('abort',cancel,{once:true});signal.throwIfAborted();
        const writing=performance.now(),output=await writeCandidate(bytes,signal,originalPrefix(bytes));
        performance.measure('pdf-save:writer',{start:writing,end:performance.now()});
        if(!(output instanceof Uint8Array)||output.byteLength<=bytes.byteLength||output.byteLength>20*1024*1024)throw {code:'writer_standards_blocked'};
        const validating=performance.now();await validateOriginal(bytes,output,signal);
        performance.measure('pdf-save:validation',{start:validating,end:performance.now()});
        signal.throwIfAborted();if(released)throw new SourceError(messages.unauthorized);
        // After commit starts, an abort cannot truthfully mean "PDF unchanged".
        signal.removeEventListener('abort',cancel);
        const committing=performance.now(),response=await bridge<unknown>('save_original_pdf',output,saveHeaders(task,bytes.byteLength,bindingId));
        performance.measure('pdf-save:ipc',{start:committing,end:performance.now()});
        // Numeric timings only: no paths, document text or persistent logs.
        const timing=ownData(response,'timings');
        for(const key of ['validationMs','backupMs','commitMs','historyMs','housekeepingMs','decodeMs','queueMs','commandMs','commitTotalMs']){const ms=ownData(timing,key);if(typeof ms==='number'&&Number.isFinite(ms)&&ms>=0&&ms<3600000)performance.measure('pdf-save:native-'+key,{start:0,duration:ms});}
        const d=ownData(response,'descriptor'),newDescriptor=d===null?null:copyDescriptor(d),hash=ownData(response,'hash');
        const pending=ownData(response,'historyPending'),reopen=ownData(response,'reopenRequired'),binding=ownData(response,'binding');
        if((d!==null&&!newDescriptor)||typeof hash!=='string'||!/^[a-f0-9]{64}$/.test(hash)||typeof pending!=='boolean'||typeof reopen!=='boolean'||reopen!==(newDescriptor===null))throw new SourceError(messages.writer_response_invalid);
        if(binding!==null&&(!['bindingId','documentId','revisionId'].every(key=>{const value=ownData(binding,key);return typeof value==='string'&&/^[a-f0-9-]{36}$/.test(value);})))throw new SourceError(messages.writer_response_invalid);
        released=true;
        performance.measure('pdf-save:total',{start:started,end:performance.now()});
        return {source:newDescriptor?desktopSource(newDescriptor,bridge,writeCandidate,validateOriginal):null,hash,
          binding:binding as import('./local-data').DocumentBinding|null,historyPending:pending,reopenRequired:reopen,cleanupPending:ownData(response,'cleanupPending')===true};
      }catch(error){
        const refused=new DesktopSourceError(error,'original'),recovery=copyDescriptor(ownData(error,'recoveryDescriptor'));
        if(recovery){released=true;refused.recoverySource=desktopSource(recovery,bridge,writeCandidate,validateOriginal);}
        throw refused;
      }
      finally{signal.removeEventListener('abort',cancel);cancel();}
    },
    async testCopy(bytes) {
      if(released||bytes.byteLength>1024*1024)throw new SourceError(messages.unauthorized);
      try {return desktopSource(await bridge<DesktopDescriptor>('save_m3_copy',{grantId:descriptor.grantId,bytes:Array.from(bytes)}),bridge);}
      catch(error){throw new SourceError(desktopError(error));}
    },
    async read(signal) {
      signal.throwIfAborted();
      if (released) throw new SourceError(messages.unauthorized);
      let taskId: string | undefined;
      const cancel = () => { if (taskId) void bridge('cancel_pdf_read', { taskId }).catch(() => {}); };
      try {
        taskId = await bridge<string>('begin_pdf_read', { grantId: descriptor.grantId });
        signal.addEventListener('abort', cancel, { once: true });
        if (signal.aborted || released) { cancel(); signal.throwIfAborted(); throw new SourceError(messages.unauthorized); }
        const bytes = await bridge<ArrayBuffer>('read_pdf', { taskId });
        signal.throwIfAborted();
        if (released) throw new SourceError(messages.unauthorized);
        if (!(bytes instanceof ArrayBuffer) || bytes.byteLength !== descriptor.size) throw new SourceError(messages.file_changed);
        return new Uint8Array(bytes);
      } catch (error) {
        signal.throwIfAborted();
        if (error instanceof SourceError) throw error;
        throw new SourceError(desktopError(error));
      } finally { signal.removeEventListener('abort', cancel); cancel(); }
    },
    async release() {
      if (released) return;
      released = true;
      // Window destruction is the backend fallback if the bridge is already gone.
      await bridge('release_pdf', { grantId: descriptor.grantId }).catch(() => {});
    },
  };
}
export async function pickDesktopPdfs(bridge: DesktopBridge = invoke) {
  const batch = await bridge<{ documents: DesktopDescriptor[]; rejected: unknown[] }>('pick_pdfs');
  return desktopBatch(batch, bridge);
}
export function desktopBatch(batch: { documents: DesktopDescriptor[]; rejected: unknown[] }, bridge: DesktopBridge = invoke) {
  return { sources: batch.documents.map(descriptor => desktopSource(descriptor, bridge)),
    warning: batch.rejected.length ? desktopError(batch.rejected[0]) : '' };
}
