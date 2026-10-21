import type { AiSlot } from './ai-client';
// The picker sits next to the send button and defaults to the main model. The
// last explicit choice is remembered per surface, so switching to the auxiliary
// model in the side chat does not change what the in-place box uses.
const KEYS: Record<string, string> = { chat: 'reader-ai-slot-chat-v1', inline: 'reader-ai-slot-inline-v1' };
export const defaultSlot: AiSlot = 'main';
export function readSlot(surface: 'chat' | 'inline'): AiSlot {
  try { return localStorage.getItem(KEYS[surface]) === 'auxiliary' ? 'auxiliary' : defaultSlot; } catch { return defaultSlot; }
}
export function writeSlot(surface: 'chat' | 'inline', slot: AiSlot): void {
  try { localStorage.setItem(KEYS[surface], slot); } catch { /* Session-only choice is safe. */ }
}
export function slotLabel(slot: AiSlot): string { return slot === 'auxiliary' ? '辅助模型' : '主模型'; }
