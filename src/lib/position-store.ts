import { validPosition } from './reading-position.ts';
import type { ReadingPosition } from './reading-position.ts';
export const POSITION_KEY = 'pdf-reader.positions.v1';
export interface PositionStorage { getItem(key: string): string | null; setItem(key: string, value: string): void; }
interface Entry { hash: string; position: ReadingPosition; updated: number; }
const validHash = (hash: string) => /^[a-f0-9]{64}$/.test(hash);
const exactKeys = (value: object, keys: string[]) => Object.keys(value).length === keys.length && Object.keys(value).every(key => keys.includes(key));
const copyPosition = (position: ReadingPosition): ReadingPosition => ({ page: position.page, zoom: position.zoom, top: position.top, left: position.left });
// Read-position convenience only. No filenames, paths, PDF bytes, annotations or history.
export class PositionStore {
  private entries: Entry[] = [];
  private writable = true;
  warning = '';
  private storage: PositionStorage | null;
  constructor(storage: PositionStorage | null) {
    this.storage = storage;
    if (!storage) { this.disable('浏览器无法长期保存位置；本次会话仍保留，刷新后请重新选择文件。'); return; }
    try {
      const raw = storage.getItem(POSITION_KEY);
      if (!raw) return;
      if (raw.length > 64 * 1024) throw Error('Oversized metadata');
      const data = JSON.parse(raw);
      if (!exactKeys(data, ['version', 'entries']) || data.version !== 1 || !Array.isArray(data.entries) || data.entries.length > 50 || !data.entries.every((entry: Entry) => entry && exactKeys(entry, ['hash', 'position', 'updated']) && validHash(entry.hash) && validPosition(entry.position) && exactKeys(entry.position, ['page', 'zoom', 'top', 'left']) && Number.isSafeInteger(entry.updated) && entry.updated >= 0) || new Set(data.entries.map((entry: Entry) => entry.hash)).size !== data.entries.length) throw Error('Unknown metadata');
      this.entries = data.entries;
    } catch { this.disable('位置记录暂时不可用；原记录保持不变，本次会话仍保留。'); }
  }
  private disable(message: string) { this.writable = false; this.warning = message; }
  get(hash: string | undefined): ReadingPosition | undefined {
    const position = this.entries.find(entry => entry.hash === hash)?.position;
    return position ? copyPosition(position) : undefined;
  }
  save(hash: string, position: ReadingPosition): boolean {
    if (!validHash(hash) || !validPosition(position)) return false;
    const old = this.entries.find(entry => entry.hash === hash);
    if (old && JSON.stringify(old.position) === JSON.stringify(position)) return true;
    this.entries = [...this.entries.filter(entry => entry.hash !== hash), { hash, position: copyPosition(position), updated: Date.now() }].slice(-50);
    if (!this.storage || !this.writable) return false;
    try { this.storage.setItem(POSITION_KEY, JSON.stringify({ version: 1, entries: this.entries })); return true; }
    catch { this.disable('浏览器无法长期保存位置；本次会话仍保留，刷新后请重新选择文件。'); return false; }
  }
}
export async function contentHash(bytes: Uint8Array, signal: AbortSignal): Promise<string> {
  signal.throwIfAborted();
  const digest = await crypto.subtle.digest('SHA-256', bytes as Uint8Array<ArrayBuffer>);
  signal.throwIfAborted();
  return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
}
