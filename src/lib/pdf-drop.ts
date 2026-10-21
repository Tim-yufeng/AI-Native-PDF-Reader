export type PdfDropHint = 'pdf' | 'invalid' | 'files' | null;
// A drag preview may hide file names. The real opening path still validates bytes.
export function pdfDropHint(names: readonly string[]): PdfDropHint {
  if (!names.length) return null;
  return names.every(name => /\.pdf$/i.test(name)) ? 'pdf' : 'invalid';
}
export function carriesFiles(transfer: DataTransfer | null): boolean {
  return !!transfer && Array.from(transfer.types).includes('Files');
}
