export type ReadingZoom = 'page-width' | 'page-fit' | number;
export interface ReadingPosition { page: number; zoom: ReadingZoom; top: number; left: number; }
export function validPosition(value: unknown): value is ReadingPosition {
  if (!value || typeof value !== 'object') return false;
  const p = value as ReadingPosition;
  return Number.isSafeInteger(p.page) && p.page >= 1 && p.page <= 2000 &&
    (p.zoom === 'page-width' || p.zoom === 'page-fit' || typeof p.zoom === 'number' && Number.isFinite(p.zoom) && p.zoom >= .25 && p.zoom <= 4) &&
    Number.isFinite(p.top) && p.top >= -2 && p.top <= 2 && Number.isFinite(p.left) && p.left >= -2 && p.left <= 2;
}
