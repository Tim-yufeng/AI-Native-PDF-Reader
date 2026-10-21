export function zoomPercent(text: string): number | null {
  if (!/^\d+(?:\.\d+)?%?$/.test(text.trim())) return null;
  const number = Number(text.trim().replace('%', ''));
  return Number.isFinite(number) && number >= 25 && number <= 400 ? number / 100 : null;
}
export type ReadingKey = 'next' | 'previous' | 'first' | 'last' | 'in' | 'out' | 'search';
export function readingKey(event: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey' | 'isComposing'>, editable: boolean): ReadingKey | null {
  if (editable || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey && event.key !== '+') return null;
  const keys: Record<string, ReadingKey> = { ArrowRight: 'next', ArrowLeft: 'previous', Home: 'first', End: 'last', '+': 'in', '=': 'in', '-': 'out', '/': 'search' };
  return keys[event.key] ?? null;
}
