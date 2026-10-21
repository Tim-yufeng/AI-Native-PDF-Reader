import type { PdfSource } from './document-source.ts';
import type { ReadingPosition } from './reading-position.ts';
import type { AnnotationDraft } from './annotation-draft.ts';
export interface DocumentTab { id: number; source: PdfSource; position?: ReadingPosition; hash?: string; draft?: AnnotationDraft; dirty?: boolean; recoveryId?: string; recoveryChecked?: boolean; }
// Tab IDs are session-local, never inferred from names, paths or PDF metadata.
export class DocumentTabs {
  tabs: DocumentTab[] = [];
  activeId: number | null = null;
  private sequence = 0;
  get active() { return this.tabs.find(tab => tab.id === this.activeId); }
  add(sources: PdfSource[]): DocumentTab | undefined {
    const added = sources.map(source => ({ id: ++this.sequence, source }));
    this.tabs = [...this.tabs, ...added];
    if (added.length) this.activeId = added.at(-1)!.id;
    return this.active;
  }
  activate(id: number) {
    if (!this.tabs.some(tab => tab.id === id) || id === this.activeId) return false;
    this.activeId = id; return true;
  }
  remember(position: ReadingPosition | null) {
    if (position && this.active) this.active.position = { ...position };
  }
  close(id: number) {
    const index = this.tabs.findIndex(tab => tab.id === id);
    if (index < 0) return false;
    const wasActive = id === this.activeId;
    this.release(this.tabs[index].source);
    this.tabs = this.tabs.filter(tab => tab.id !== id);
    if (wasActive) this.activeId = this.tabs[Math.min(index, this.tabs.length - 1)]?.id ?? null;
    return wasActive;
  }
  private release(source: PdfSource) {
    try { void Promise.resolve(source.release?.()).catch(() => {}); } catch { /* Backend window exit also revokes grants. */ }
  }
  clear() {
    for (const tab of this.tabs) this.release(tab.source);
    this.tabs = []; this.activeId = null;
  }
}
