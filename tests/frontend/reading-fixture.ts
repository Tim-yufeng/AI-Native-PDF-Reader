// Original synthetic controls: ASCII text, varied sizes/rotation, or image-only.
// In-memory generator; tests never modify user documents or G1 controls.
export function readingFixture(count = 120, imageOnly = false, labels: Record<number, string> = {}): Uint8Array {
  if (!Number.isInteger(count) || count < 1 || count > 2000) throw new Error('Invalid fixture count');
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    `<< /Type /Pages /Count ${count} /Kids [${Array.from({ length: count }, (_, index) => `${4 + index * 2} 0 R`).join(' ')}] >>`,
    imageOnly ? '<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length 25 >>\nstream\nff000000ff000000ff000000>\nendstream'
      : '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  for (let number = 1; number <= count; number++) {
    const [width, height] = number % 3 === 0 ? [792, 612] : number % 3 === 2 ? [420, 640] : [612, 792];
    const body = imageOnly ? 'q 300 0 0 400 50 100 cm /Im1 Do Q'
      : `BT /F1 20 Tf 40 ${height - 70} Td (${(labels[number] ?? `Reading page ${number} - needle first`).replace(/[()\\]/g, '\\$&')}) Tj 0 -40 Td (needle repeat. ABC 0123456789) Tj ET`;
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${width} ${height}] /CropBox [18 18 ${width - 18} ${height - 18}] /Rotate ${number % 7 === 0 ? 90 : 0} /Resources << ${imageOnly ? '/XObject << /Im1 3 0 R >>' : '/Font << /F1 3 0 R >>'} >> /Contents ${5 + (number - 1) * 2} 0 R >>`);
    objects.push(`<< /Length ${body.length} >>\nstream\n${body}\nendstream`);
  }
  let pdf = '%PDF-1.7\n'; const offsets: number[] = [];
  objects.forEach((value, index) => { offsets.push(pdf.length); pdf += `${index + 1} 0 obj\n${value}\nendobj\n`; });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.map(offset => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(pdf);
}
