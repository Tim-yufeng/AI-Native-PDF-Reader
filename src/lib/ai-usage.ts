/** Only display reported counts. Missing usage is never estimated as zero. */
export function tokenUsage(usage: Record<string, unknown> | null | undefined): string {
  if (usage?.mock === true) return '本地 Mock · 无 API 消耗';
  const count = (key: string): number | null => {
    const value = usage?.[key];
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : null;
  };
  const input = count('promptTokens'), output = count('completionTokens');
  const sum = input !== null && output !== null && Number.isSafeInteger(input + output) ? input + output : null;
  const total = count('totalTokens') ?? sum;
  if (input === null && output === null && total === null) return 'Token：未返回用量';
  const format = (n: number | null) => n === null ? '—' : n.toLocaleString('zh-CN');
  return `Token：输入 ${format(input)} · 输出 ${format(output)} · 合计 ${format(total)}`;
}
