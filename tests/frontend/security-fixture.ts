// Synthetic, disposable read-only viewer control. The action may be parsed but
// must never execute; the external URI must remain disabled in the GUI.
export function securityFixture(invalidPage = false): Uint8Array {
  const body = 'BT /F1 18 Tf 60 720 Td (M2 READ-ONLY SAFETY CONTROL) Tj 0 -40 Td (Disabled external link below) Tj ET';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R /OpenAction 7 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    invalidPage ? 'false' : '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R /Annots [6 0 R] >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    `<< /Length ${body.length} >>\nstream\n${body}\nendstream`,
    '<< /Type /Annot /Subtype /Link /Rect [60 665 350 700] /Border [0 0 1] /A << /S /URI /URI (https://example.invalid/m2-a1-link) >> >>',
    '<< /S /JavaScript /JS (app.alert("M2_SCRIPT_MUST_NOT_RUN")) >>',
  ];
  let pdf = '%PDF-1.7\n'; const offsets = [0];
  objects.forEach((value, index) => { offsets.push(pdf.length); pdf += `${index + 1} 0 obj\n${value}\nendobj\n`; });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.slice(1).map(offset => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(pdf);
}
