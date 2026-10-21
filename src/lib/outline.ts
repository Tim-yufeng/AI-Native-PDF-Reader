export interface Bookmark { title: string; destination: string | unknown[] | null; depth: number; }
// Untrusted PDF outline: bounded traversal and plain text, never URL navigation.
export function bookmarks(outline: unknown): Bookmark[] {
  const result: Bookmark[] = [];
  const visit = (items: unknown, depth: number) => {
    if (!Array.isArray(items) || depth > 20) return;
    for (const item of items) {
      if (result.length >= 2000) return;
      if (!item || typeof item !== 'object') continue;
      const entry = item as { title?: unknown; dest?: unknown; items?: unknown };
      const destination = typeof entry.dest === 'string' ? entry.dest.slice(0, 4096)
        : Array.isArray(entry.dest) && entry.dest.length <= 10 ? entry.dest : null;
      result.push({ title: typeof entry.title === 'string' ? entry.title.slice(0, 512) : '未命名书签', destination, depth });
      visit(entry.items, depth + 1);
    }
  };
  visit(outline, 0); return result;
}
