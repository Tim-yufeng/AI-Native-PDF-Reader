export interface SendKey { key: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean; repeat?: boolean; keyCode?: number; }
export function enterSends(event: SendKey, composing = false, recentlyComposed = false): boolean {
  return event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing && !composing && !recentlyComposed && event.keyCode !== 229 && !event.repeat;
}
export const sourceReason = (reasons: string[] = []): string => reasons.map(reason => ({manual:'本轮选页',region:'本轮区域',pinned:'固定页面',current:'当前页',neighbor:'邻近页',retrieved:'全文检索'}[reason] ?? reason)).join(' · ') || '历史来源';
