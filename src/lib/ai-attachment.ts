// Screenshots and pictures the user attaches from outside the PDF. They are read
// only from an explicit file choice or an explicit paste, are re-encoded to a
// bounded PNG in this process, and are never written to the local index or to
// any cache: they exist for the one round that carries them.
export interface AttachedImage { id: string; name: string; png: string; width: number; height: number; }

// Bounds shared with the backend: 3 MiB per image, 8 MiB and 8 images per round.
export const IMAGE_BYTES_MAX = 8 * 1024 * 1024;
export const IMAGE_SINGLE_MAX = 3 * 1024 * 1024;
export const IMAGE_COUNT_MAX = 8;
const LONG_EDGE_MAX = 1600;

export function imageBytes(png: string): number {
  const body = png.slice(png.indexOf(',') + 1);
  // 4 base64 characters carry 3 bytes; padding is not significant enough to matter here.
  return Math.floor(body.length * 3 / 4);
}

export function acceptedImage(file: File): boolean {
  return /^image\/(png|jpeg|webp|gif|bmp)$/i.test(file.type);
}

/** Re-encode to a bounded PNG so a large screenshot cannot exceed the budget. */
export async function toAttachedImage(file: File | Blob, name: string): Promise<AttachedImage> {
  const type = (file as File).type || '';
  if (type && !acceptedImage(file as File)) throw Error('只支持 PNG、JPEG、WebP、GIF 或 BMP 图片。');
  const bitmap = await createImageBitmap(file);
  try {
    const scale = Math.min(1, LONG_EDGE_MAX / Math.max(bitmap.width, bitmap.height));
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = document.createElement('canvas');
    canvas.width = width; canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) throw Error('本机无法处理这张图片。');
    context.drawImage(bitmap, 0, 0, width, height);
    const png = canvas.toDataURL('image/png');
    canvas.width = 0; canvas.height = 0;
    if (imageBytes(png) > IMAGE_SINGLE_MAX) throw Error('这张图片即使压缩后仍超出单张上限，请先裁剪。');
    return { id: crypto.randomUUID(), name: name.slice(0, 80), png, width, height };
  } finally { bitmap.close?.(); }
}

export function addImage(current: AttachedImage[], next: AttachedImage): AttachedImage[] {
  const kept = [...current, next];
  while (kept.length > IMAGE_COUNT_MAX) kept.shift();
  let total = kept.reduce((sum, item) => sum + imageBytes(item.png), 0);
  while (kept.length > 1 && total > IMAGE_BYTES_MAX) total -= imageBytes(kept.shift()!.png);
  return kept;
}

/** Images carried by a paste event, in the order the user supplied them. */
export function clipboardImages(data: DataTransfer | null): (File | Blob)[] {
  if (!data) return [];
  const files: (File | Blob)[] = [];
  // `items` is what a paste carries; `files` is the fallback some builds use.
  for (const item of Array.from(data.items ?? [])) {
    if (item.kind !== 'file') continue;
    const file = item.getAsFile();
    if (file && acceptedImage(file)) files.push(file);
  }
  if (!files.length) for (const file of Array.from(data.files ?? [])) if (acceptedImage(file)) files.push(file);
  return files;
}

/** Images carried by a drop event. A URL dragged from a browser is not an image
 *  we can read, so it is refused rather than fetched. */
export function droppedImages(data: DataTransfer | null): (File | Blob)[] {
  const files = clipboardImages(data);
  if (!files.length && (data?.types ?? []).includes('text/uri-list')) return [];
  return files;
}

/** True when a drop or paste actually carries image files, so the caller may
 *  claim the event; otherwise it must let the default behaviour through. */
export function carriesImage(data: DataTransfer | null): boolean {
  if (!data) return false;
  return Array.from(data.items ?? []).some(item => item.kind === 'file' && (!item.type || /^image\//i.test(item.type)))
    || Array.from(data.files ?? []).some(acceptedImage);
}

