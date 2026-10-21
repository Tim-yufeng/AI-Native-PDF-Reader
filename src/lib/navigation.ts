export function pageNumber(text: string, total: number): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const value = Number(text.trim());
  return Number.isSafeInteger(value) && value >= 1 && value <= total ? value : null;
}

export const THUMBNAIL_ROW = 180;
export function thumbnailWindow(top: number, height: number, total: number) {
  const start = Math.max(0, Math.floor(top / THUMBNAIL_ROW) - 2);
  const end = Math.min(total, Math.ceil((top + height) / THUMBNAIL_ROW) + 2);
  return Array.from({ length: Math.max(0, end - start) }, (_, index) => start + index + 1);
}
