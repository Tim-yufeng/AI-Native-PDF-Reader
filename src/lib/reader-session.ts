import { validateSource, readableError, SourceError, MAX_INPUT_BYTES } from './document-source.ts';
import type { PdfSource } from './document-source.ts';

export interface ReaderState {
  status: 'idle' | 'loading' | 'ready' | 'error';
  name: string;
  pageCount: number;
  error: string;
}
export const emptyReaderState = (): ReaderState => ({ status: 'idle', name: '', pageCount: 0, error: '' });
export interface LoadingTask<T> { promise: Promise<T>; destroy(): Promise<void>; }

// One document at a time. Superseded reads cannot attach to the active view.
export class ReaderSession<T extends { numPages: number }> {
  private generation = 0;
  private abort: AbortController | null = null;
  private task: LoadingTask<T> | null = null;
  private disposed = false;
  private stops = new WeakMap<LoadingTask<T>, Promise<void>>();
  private readonly load: (bytes: Uint8Array) => LoadingTask<T>;
  private readonly stateChanged: (state: ReaderState) => void;
  private readonly documentChanged: (document: T | null) => void;
  constructor(
    load: (bytes: Uint8Array) => LoadingTask<T>,
    stateChanged: (state: ReaderState) => void,
    documentChanged: (document: T | null) => void,
  ) { this.load = load; this.stateChanged = stateChanged; this.documentChanged = documentChanged; }

  private stop(task: LoadingTask<T> | null): Promise<void> {
    if (!task) return Promise.resolve();
    let pending = this.stops.get(task);
    if (!pending) { pending = task.destroy(); this.stops.set(task, pending); }
    return pending;
  }

  async open(source: PdfSource): Promise<void> {
    if (this.disposed) return;
    const generation = ++this.generation;
    this.abort?.abort();
    const abort = this.abort = new AbortController();
    const previous = this.task;
    this.task = null;
    this.documentChanged(null);
    this.stateChanged({ status: 'loading', name: source.name, pageCount: 0, error: '' });
    let task: LoadingTask<T> | null = null;
    try {
      await this.stop(previous);
      abort.signal.throwIfAborted();
      validateSource(source);
      const bytes = await source.read(abort.signal);
      abort.signal.throwIfAborted();
      if (bytes.length !== source.size || bytes.length > MAX_INPUT_BYTES) {
        throw new SourceError('读取的文件大小不一致，请重新打开文件。');
      }
      task = this.load(bytes);
      this.task = task;
      const document = await task.promise;
      if (generation !== this.generation || this.disposed) { await this.stop(task); return; }
      if (!Number.isSafeInteger(document.numPages) || document.numPages < 1 || document.numPages > 2000) {
        throw new SourceError('页数无效或超过本阶段 2000 页的保护限制。');
      }
      this.documentChanged(document);
      this.stateChanged({ status: 'ready', name: source.name, pageCount: document.numPages, error: '' });
    } catch (error) {
      await this.stop(task).catch(() => {});
      if (generation !== this.generation || this.disposed || abort.signal.aborted) return;
      this.task = null;
      this.documentChanged(null);
      this.stateChanged({ status: 'error', name: source.name, pageCount: 0, error: readableError(error) });
    }
  }

  async close(): Promise<void> {
    const generation = ++this.generation;
    this.abort?.abort();
    this.abort = null;
    const task = this.task;
    this.task = null;
    this.documentChanged(null);
    if (!this.disposed) this.stateChanged(emptyReaderState());
    await this.stop(task).catch(() => {
      if (!this.disposed && generation === this.generation) this.stateChanged({ ...emptyReaderState(),
        error: '文件已从界面关闭，但部分阅读资源未能释放。请刷新页面后再打开。' });
    });
  }

  async dispose(): Promise<void> { this.disposed = true; await this.close(); }
}
