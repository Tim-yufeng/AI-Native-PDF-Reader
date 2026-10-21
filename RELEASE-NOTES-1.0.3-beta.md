# AI-Native PDF Reader 1.0.3 閳?Public Beta release notes

First public beta. Windows 10/11 x64. Unsigned installer. Licensed AGPL-3.0-or-later.

## What this release is

A Windows PDF reader for reading and explaining course-style material. It writes real
PDF annotations in an appended revision, keeps a local full-text index, and can explain
a selected passage or a dragged region using a model you configure yourself.

## Highlights

- **Native annotations.** Highlight, freehand ink and text boxes become standard PDF
  annotations. The original is never truncated; a failed save leaves it untouched and
  keeps the draft. Six colours per stroke tool, remembered per tool.
- **In-place explanation.** Select text or drag a region and ask; the answer appears in
  a floating box, and the side chat keeps a durable history.
- **Local retrieval.** The document is indexed on this machine. What you select or
  annotate is placed first and never dropped; automatic retrieval fills the rest and
  reports when it had to be trimmed.
- **Styles and models.** Import your own writing styles and pick one; configure a main
  and an auxiliary model separately.
- **Reading.** Tabs, thumbnails, outline, search, zoom and restored reading position.

## Requirements

- Windows 10 or 11, x64.
- WebView2 runtime. Windows 11 ships it; on Windows 10 install Microsoft's runtime if it
  is missing. The installer does not bundle a runtime.
- Your own API key for model features. Without one the app still reads, annotates and
  saves, and the Mock model can be used to explore the UI.

## Install

1. Download `AI-Native PDF Reader_1.0.3_x64-setup.exe`.
2. Compare its SHA-256 with the value published beside it.
3. Run it. The installer is unsigned, so Windows may warn about an unknown publisher.
4. Launch and open a PDF.

## Data

Settings live in `%APPDATA%\io.github.ainativepdfreader\`; documents, index, chat and
backups live in `%LOCALAPPDATA%\io.github.ainativepdfreader\data-manager\`. Nothing is
uploaded unless you ask a question, and then only the content of that question.

## Known limitations

- Unsigned installer; SmartScreen may warn.
- PDFs close to 16 MB are refused at save time for size, keeping the original and draft.
- External-reader compatibility (Edge, Adobe) is only partly verified: annotations
  display, but full editing round-trips in those readers are not claimed.
- Scanned pages without a text layer are treated as images, not searchable text.
- Colours and font metrics can differ slightly in other readers.
- Windows only; no macOS or Linux build.

## Licence

AGPL-3.0-or-later, because the app ships MuPDF.js. The full text is in `LICENSE`;
third-party components are listed in `THIRD-PARTY-NOTICES.md`.

## Not included

No telemetry, no account system, no background model calls, no auto-update.
