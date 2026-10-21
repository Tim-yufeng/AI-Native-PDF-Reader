// Colour choices for the two stroke tools. Kept out of App.svelte so the defaults,
// the validation and the remembered choice can be tested without a browser.
export type StrokeKind = 'highlight' | 'ink';
export interface ToolColor { color: string; name: string; }

/** Six choices per tool; the first one is the default the reader has always used. */
export const toolPalette: Record<StrokeKind, readonly ToolColor[]> = {
  highlight: [
    { color: '#FFFF00', name: '黄' }, { color: '#7BE38A', name: '绿' }, { color: '#7FD4F0', name: '蓝' },
    { color: '#FFA6D5', name: '粉' }, { color: '#FFB86B', name: '橙' }, { color: '#C9A6FF', name: '紫' },
  ],
  ink: [
    { color: '#000000', name: '黑' }, { color: '#D0342C', name: '红' }, { color: '#2C6FD0', name: '蓝' },
    { color: '#2E8B57', name: '绿' }, { color: '#E08A2E', name: '橙' }, { color: '#7A4FA0', name: '紫' },
  ],
};

export const defaultToolColors = (): Record<StrokeKind, string> => ({ highlight: '#FFFF00', ink: '#000000' });

/** A colour is usable only if it is one of this tool's own choices. */
export function validToolColor(kind: StrokeKind, value: unknown): value is string {
  return typeof value === 'string' && toolPalette[kind].some(option => option.color.toLowerCase() === value.toLowerCase());
}

export const TOOL_COLOR_KEY = 'reader-tool-colors';

/** Restores remembered colours, ignoring anything unknown or malformed. */
export function parseToolColors(raw: string | null): Record<StrokeKind, string> {
  const colors = defaultToolColors();
  if (!raw) return colors;
  let parsed: unknown;
  try { parsed = JSON.parse(raw); } catch { return colors; }
  if (!parsed || typeof parsed !== 'object') return colors;
  for (const kind of ['highlight', 'ink'] as const) {
    const value = (parsed as Record<string, unknown>)[kind];
    if (validToolColor(kind, value)) colors[kind] = value;
  }
  return colors;
}

/** The value written to storage; only the two known tools, never anything else. */
export function serializeToolColors(colors: Record<StrokeKind, string>): string {
  return JSON.stringify({ highlight: colors.highlight, ink: colors.ink });
}
