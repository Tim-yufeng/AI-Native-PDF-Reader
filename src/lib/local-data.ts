import { invoke } from '@tauri-apps/api/core';
import type { DesktopBridge } from './desktop-source.ts';
import { validPosition, type ReadingPosition } from './reading-position.ts';
import { validateDraft, type AnnotationDraft } from './annotation-draft.ts';

export interface DataStatus { available: boolean; path: string; areas: string[]; acl: string; generation: number; error?: string; }
export interface LocalSettings { schemaVersion: 1; thumbnailOpen: boolean; cacheBytes: number; }
export interface DocumentBinding { bindingId: string; documentId: string; revisionId: string; position?: ReadingPosition; identityNotice?: string; }
export interface HistoryMessage {
  id: string; parent?: string | null; editedFrom?: string | null; content: string;
  status: string; role: 'user' | 'assistant'; createdMs: number; page?: number | null; deleted: boolean; truncated: boolean;
  metadata: { provider?: string | null; model?: string | null; preset_version?: string | null; request_id?: string | null; sent_context_hash?: string | null; };
}
export interface ChatVersion {messageId:string;index:number;total:number;previousId:string|null;nextId:string|null;}
export interface ChatSession {id:string;title:string;archived:boolean;leafId:string;}
export interface ChatView {sessionId:string|null;leafId:string|null;sessions:ChatSession[];path:HistoryMessage[];versions:ChatVersion[];}
export interface LocalRecovery { id: string; draft: AnnotationDraft; updated: number; }
export interface LocalBackup { id: string; createdMs: number; bytes: number; }
// A usable store can still carry a recovery warning. Availability must never
// silently erase the fact that startup fell back to an earlier data directory.
export function dataStatusWarning(status: DataStatus | null): string { return status?.error || ''; }
export function chatUnavailableReason(status:DataStatus|null,bindingError:string,hasPdf:boolean):string {
  if(!status)return '正在初始化本地数据，请稍候。';
  if(!status.available)return (status.error||'本地数据初始化未完成，聊天与原位解释已暂停。')+(status.path?` 数据位置：${status.path}`:'');
  if(bindingError)return bindingError;
  return hasPdf?'正在验证 PDF 身份并恢复本地历史，请稍候。':'请先打开桌面 PDF，再开始提问。';
}
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value);
const invalid = () => Error('本地数据返回不完整，已有记录保持不变。');
export function localDataError(error: unknown): string {
  const code = error && typeof error === 'object' && 'code' in error ? String(error.code) : '';
  if (code === 'unauthorized') return '此文档授权已失效，请关闭后重新打开。';
  if (code === 'cancelled') return '操作已取消，原数据保持不变。';
  if (code === 'busy') return '本地数据暂时忙碌，请稍后重试。';
  if (code === 'document_identity_unavailable') return '当前 PDF 所在位置不支持可靠文件身份，聊天与原位解释已暂停；请从本地磁盘重新打开 PDF。不会按文件名合并历史。';
  if (code === 'file_changed') return 'PDF 内容或身份已改变，聊天绑定已暂停；请关闭此文档后重新打开。';
  if (code === 'database_unavailable') return '读取或初始化此文档的数据库失败；已有历史保留，请检查本地数据状态。';
  if (code === 'data_refused') return '此文档的历史绑定未通过安全或完整性检查，未合并其他文档记录；请检查本地数据状态。';
  return '本地数据操作未完成，原 PDF 未修改；请重试并保留当前窗口。';
}

// All file paths and identity decisions stay behind the desktop bridge. This
// client only carries grants/bindings; there is no arbitrary path or SQL input.
export class LocalData {
  private bridge: DesktopBridge;
  constructor(bridge: DesktopBridge = invoke) { this.bridge = bridge; }
  async status(): Promise<DataStatus> { return this.bridge<DataStatus>('m4_status'); }
  private chatValue(value:ChatView):ChatView {
    if(!value||!Array.isArray(value.path)||!Array.isArray(value.sessions)||!Array.isArray(value.versions)||value.sessions.length>4096||value.path.length>4096||(value.sessionId!==null&&!uuid(value.sessionId))||(value.leafId!==null&&!uuid(value.leafId))||!value.sessions.every(s=>uuid(s.id)&&uuid(s.leafId)&&typeof s.title==='string'&&typeof s.archived==='boolean')||!value.path.every(r=>uuid(r.id)&&typeof r.content==='string'&&!!r.metadata)||!value.versions.every(v=>uuid(v.messageId)&&Number.isSafeInteger(v.index)&&Number.isSafeInteger(v.total)&&v.index>=1&&v.index<=v.total&&(v.previousId===null||uuid(v.previousId))&&(v.nextId===null||uuid(v.nextId))))throw invalid();
    return value;
  }
  async chatView(bindingId:string):Promise<ChatView>{this.binding(bindingId);return this.chatValue(await this.bridge<ChatView>('m5_chat_view',{bindingId}));}
  async chatSelect(bindingId:string,messageId:string|null):Promise<ChatView>{this.binding(bindingId);if(messageId!==null)this.message(messageId);return this.chatValue(await this.bridge<ChatView>('m5_chat_select',{bindingId,messageId}));}
  async chatArchive(bindingId:string,sessionId:string,archived:boolean):Promise<ChatView>{this.binding(bindingId);this.message(sessionId);return this.chatValue(await this.bridge<ChatView>('m5_chat_archive',{bindingId,sessionId,archived}));}
  async chatDelete(bindingId:string,sessionId:string):Promise<boolean>{this.binding(bindingId);this.message(sessionId);const deleted=await this.bridge<boolean>('m5_chat_delete',{bindingId,sessionId});if(typeof deleted!=='boolean')throw invalid();return deleted;}
  async bind(grantId: string, pageCount: number): Promise<DocumentBinding> {
    if (!uuid(grantId) || !Number.isSafeInteger(pageCount) || pageCount < 1 || pageCount > 2000) throw invalid();
    const result = await this.bridge<DocumentBinding>('m4_bind_document', { grantId, pageCount });
    if (!result || !uuid(result.bindingId) || !uuid(result.documentId) || !uuid(result.revisionId) || result.position != null && !validPosition(result.position)) throw invalid();
    return result;
  }
  async messages(bindingId: string): Promise<HistoryMessage[]> {
    this.binding(bindingId);
    const records = await this.bridge<HistoryMessage[]>('m4_messages', { bindingId });
    if (!Array.isArray(records) || records.length > 100 || !records.every(record => record && uuid(record.id) &&
      (record.parent == null || uuid(record.parent)) && (record.editedFrom == null || uuid(record.editedFrom)) &&
      typeof record.content === 'string' && record.content.length <= 1024 * 1024 &&
      ['user', 'assistant'].includes(record.role) && ['streaming', 'complete', 'cancelled', 'failed'].includes(record.status) &&
      Number.isSafeInteger(record.createdMs) && record.createdMs >= 0 && typeof record.deleted === 'boolean' && typeof record.truncated === 'boolean' &&
      (record.page == null || Number.isSafeInteger(record.page) && record.page >= 1 && record.page <= 2000) &&
      record.metadata && typeof record.metadata === 'object')) throw invalid();
    return records;
  }
  async add(bindingId: string, content: string, page: number, parentId?: string): Promise<string> {
    this.binding(bindingId); this.content(content);
    if (!Number.isSafeInteger(page) || page < 1 || page > 2000 || parentId != null && !uuid(parentId)) throw invalid();
    return this.bridge<string>('m4_add_message', { bindingId, content, page, parentId: parentId ?? null });
  }
  async edit(bindingId: string, messageId: string, content: string): Promise<string> {
    this.binding(bindingId); this.message(messageId); this.content(content);
    return this.bridge<string>('m4_edit_prompt', { bindingId, messageId, content });
  }
  async generate(bindingId: string, parentId: string): Promise<string> {
    this.binding(bindingId); this.message(parentId);
    return this.bridge<string>('m4_generate_synthetic', { bindingId, parentId });
  }
  async hide(bindingId: string, messageId: string): Promise<void> {
    this.binding(bindingId); this.message(messageId);
    await this.bridge('m4_delete_message', { bindingId, messageId });
  }
  async deleteDocumentHistory(bindingId: string): Promise<boolean> {
    this.binding(bindingId);
    const deleted = await this.bridge<boolean>('m4_delete_document_history', { bindingId });
    if (typeof deleted !== 'boolean') throw invalid();
    return deleted;
  }
  async position(bindingId: string, position: ReadingPosition): Promise<void> {
    this.binding(bindingId); if (!validPosition(position)) throw invalid();
    await this.bridge('m4_position', { bindingId, position });
  }
  async release(bindingId: string): Promise<void> { this.binding(bindingId); await this.bridge('m4_release_binding', { bindingId }); }
  async drafts(bindingId: string): Promise<LocalRecovery[]> {
    this.binding(bindingId);
    const records = await this.bridge<{ id: string; json: string; updated: number }[]>('m4_list_drafts', { bindingId });
    if (!Array.isArray(records) || records.length > 64) throw invalid();
    return records.map(record => {
      if (!uuid(record.id) || typeof record.json !== 'string' || record.json.length > 1024 * 1024 || !Number.isSafeInteger(record.updated) || record.updated < 0) throw invalid();
      return { id: record.id, updated: record.updated, draft: validateDraft(JSON.parse(record.json)) };
    });
  }
  async putDraft(bindingId: string, id: string, draft: AnnotationDraft): Promise<void> {
    this.binding(bindingId); this.message(id);
    const json = JSON.stringify(validateDraft(draft));
    if (new TextEncoder().encode(json).byteLength > 256 * 1024) throw invalid();
    await this.bridge('m4_put_draft', { bindingId, id, json });
  }
  async dropDraft(bindingId: string, id: string): Promise<void> { this.binding(bindingId); this.message(id); await this.bridge('m4_drop_draft', { bindingId, id }); }
  async backup(): Promise<string> { return this.bridge<string>('m4_backup'); }
  async backups(): Promise<LocalBackup[]> {
    const backups = await this.bridge<LocalBackup[]>('m4_list_backups');
    if (!Array.isArray(backups) || backups.length > 128 || !backups.every(backup => uuid(backup.id) && Number.isSafeInteger(backup.createdMs) && backup.createdMs >= 0 && Number.isSafeInteger(backup.bytes) && backup.bytes >= 0)) throw invalid();
    return backups;
  }
  async restoreBackup(backupId: string): Promise<DataStatus | null> { this.message(backupId); return this.bridge<DataStatus | null>('m4_restore_backup', { backupId }); }
  async migrate(): Promise<DataStatus | null> { return this.bridge<DataStatus | null>('m4_pick_migrate_data'); }
  async clearCache(): Promise<void> { await this.bridge('m4_cache_clear'); }
  async settings(): Promise<LocalSettings> { return this.validateSettings(await this.bridge<LocalSettings>('m4_get_settings')); }
  async updateSettings(thumbnailOpen: boolean, cacheBytes: number): Promise<LocalSettings> {
    this.validateSettings({ schemaVersion: 1, thumbnailOpen, cacheBytes });
    return this.validateSettings(await this.bridge<LocalSettings>('m4_set_settings', { thumbnailOpen, cacheBytes }));
  }
  private validateSettings(settings: LocalSettings) {
    if (!settings || settings.schemaVersion !== 1 || typeof settings.thumbnailOpen !== 'boolean' ||
      !Number.isSafeInteger(settings.cacheBytes) || settings.cacheBytes < 0 || settings.cacheBytes > 512 * 1024 * 1024) throw invalid();
    return settings;
  }
  private binding(id: string) { if (!uuid(id)) throw invalid(); }
  private message(id: string) { if (!uuid(id)) throw invalid(); }
  private content(text: string) { if (!text.trim() || new TextEncoder().encode(text).byteLength > 16 * 1024) throw Error('记录为空或超过 16 KB，请缩短文字。'); }
}

// A late history query must never update the newly active document. The same
// gate also cancels outstanding results when its panel/window is disposed.
export class HistoryRequestGate {
  private epoch = 0;
  private active: string | null = null;
  select(bindingId: string | null) { this.active = bindingId; this.epoch++; }
  async run<T>(bindingId: string, request: () => Promise<T>): Promise<{ current: boolean; value: T }> {
    const epoch = ++this.epoch;
    const value = await request();
    return { current: epoch === this.epoch && bindingId === this.active, value };
  }
}
