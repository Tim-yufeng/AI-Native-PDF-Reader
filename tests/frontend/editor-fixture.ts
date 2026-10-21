// Small original PDF controls for editor geometry. Generated in memory only;
// never reads, saves or mutates a user's document or the G1 control fixtures.
export interface EditorFixturePage {
  rotation: 0 | 90 | 180 | 270;
  media: readonly [number, number, number, number];
  crop: readonly [number, number, number, number];
  dark: boolean;
}

export const editorFixturePages: readonly EditorFixturePage[] = [
  { rotation: 0, media: [0, 0, 612, 792], crop: [18, 18, 594, 774], dark: false },
  { rotation: 90, media: [-32, 16, 448, 700], crop: [-20, 28, 432, 676], dark: true },
  { rotation: 180, media: [40, -30, 832, 582], crop: [58, -12, 810, 560], dark: false },
  { rotation: 270, media: [-64, -96, 496, 824], crop: [-52, -78, 480, 806], dark: true },
];

export function editorFixture(label = 'Original synthetic editor geometry'): Uint8Array {
  // ASCII-only PDF source keeps byte offsets identical to string offsets.
  if (!/^[\x20-\x7e]{1,80}$/.test(label)) throw Error('Synthetic fixture label must be bounded ASCII');
  const literal = label.replace(/[()\\]/g, '\\$&');
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    `<< /Type /Pages /Count 4 /Kids [4 0 R 6 0 R 8 0 R 10 0 R] >>`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  for (const [index, page] of editorFixturePages.entries()) {
    const [left, bottom, right, top] = page.media;
    const background = page.dark ? '0.075 0.09 0.11' : '1 1 1';
    const foreground = page.dark ? '1 1 1' : '0.12 0.18 0.25';
    const stream = `q ${background} rg ${left} ${bottom} ${right - left} ${top - bottom} re f Q\n` +
      `BT /F1 14 Tf ${foreground} rg ${page.crop[0] + 24} ${page.crop[3] - 36} Td ` +
      `(${literal} - page ${index + 1}, rotation ${page.rotation}) Tj ET\n` +
      `q 0.35 0.48 0.65 RG 0.5 w ${page.crop[0] + 12} ${page.crop[1] + 12} ` +
      `${page.crop[2] - page.crop[0] - 24} ${page.crop[3] - page.crop[1] - 24} re S Q`;
    const contents = 5 + index * 2;
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [${page.media.join(' ')}] ` +
      `/CropBox [${page.crop.join(' ')}] /Rotate ${page.rotation} ` +
      `/Resources << /Font << /F1 3 0 R >> >> /Contents ${contents} 0 R >>`);
    objects.push(`<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`);
  }
  let pdf = '%PDF-1.7\n';
  const offsets: number[] = [];
  for (const [index, object] of objects.entries()) {
    offsets.push(pdf.length);
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.map(offset => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(pdf);
}
