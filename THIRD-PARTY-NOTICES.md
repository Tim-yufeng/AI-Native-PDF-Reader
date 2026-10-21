# Third-party notices

This project is released under **AGPL-3.0-or-later** (see `LICENSE`). The components
below are bundled or linked; the AGPL obligation for the distribution as a whole comes
from MuPDF.js.

| Component | Version | Licence | How it is used | Licence text |
| --- | --- | --- | --- | --- |
| MuPDF.js (`mupdf` npm package, MuPDF compiled to WebAssembly) | 1.28.1 | **AGPL-3.0-or-later** | PDF writer: annotation appearance streams, font embedding, appended revisions | `node_modules/mupdf/LICENSE`; the version and licence are also written to `public/mupdf/version.json` at build time |
| PDF.js (`pdfjs-dist`) | 6.4.299 | Apache-2.0 | Reader: rendering, text layer, annotation editors | `node_modules/pdfjs-dist/LICENSE` |
| Noto Sans SC, static Regular v41 | v41 | SIL Open Font License 1.1 | Pinned Unicode face embedded into text-box appearances | `worker/fonts/NotoSansSC-v41-OFL.txt`, verified against `worker/freetext-font.json` |
| Tauri (framework, CLI, runtime) | 2.x | MIT OR Apache-2.0 | Desktop shell, IPC, bundling | crate and npm package metadata |
| WebView2 runtime (Microsoft Edge WebView2) | system-provided | Microsoft terms for the redistributable runtime | Renderer host. Not bundled; the installer requires the runtime to be present | Microsoft's terms apply; review them before distributing a bundle that carries the runtime |
| Svelte | 5.x | MIT | UI | npm package |
| Vite | 6.x | MIT | Bundler | npm package |
| KaTeX | 0.16.x | MIT | Maths rendering | npm package |
| marked | 15.x | MIT | Markdown rendering | npm package |
| DOMPurify | 3.x | Apache-2.0 OR MPL-2.0 | Sanitising model output | npm package |

## Why AGPL

MuPDF.js is AGPL-3.0-or-later. Shipping it inside the application makes the combined
work subject to the AGPL, so the project itself is licensed AGPL-3.0-or-later rather
than under a permissive licence. Anyone distributing a build, or running a modified
version as a network service, must pass on the same freedoms and offer the
corresponding source.

If you need a different licence, the alternatives are a commercial MuPDF licence from
Artifex, or removing the MuPDF.js writer (and with it the save feature).

## Full dependency lists

- Rust crates: `src-tauri/Cargo.lock`
- npm packages: `package-lock.json`

Those files are authoritative for versions. This notice covers the components that are
redistributed or whose licences carry obligations.
