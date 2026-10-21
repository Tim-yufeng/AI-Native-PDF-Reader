# AI-Native PDF Reader

**Read a paper or a lecture handout with a tutor beside you.** Select a line, ask in
place, and get an explanation that quotes the page it came from. Every annotation,
conversation and reading position stays on your machine.

English | [简体中文](README.zh-CN.md)

---

## Why it exists

Reading dense material is slow in a way that has nothing to do with reading speed. You
stop at a symbol, a skipped step, a sentence that assumes something you never learned.
Highlighting does not answer the question, and switching to a chat window breaks the
thread.

This reader keeps the question where you are. Drag over the confusing part, ask, and
the answer appears next to it — short by default, expandable when it matters.

## What you can do

**Ask about exactly what you highlighted.** Select text or drag a region on the page.
The answer is grounded in that page, and it tells you which source it used, so you can
check it instead of trusting it.

**Read the answer where the question came from.** Explanations appear in a small box
beside the passage instead of replacing it; you can keep reading with the explanation
visible, then confirm it into the conversation or dismiss it.

**Keep your taste in the writing.** Import your own explanation styles — the phrasing
you actually find clear — and pick one for a session. It changes how the answer sounds;
it never changes the facts, the maths or the citation.

**Annotate the way you already do.** Highlight, draw with ink, or add a text box. They
are standard PDF annotations, saved into your file, and they still open and display in
ordinary PDF readers.

**Come back to where you were.** Tabs, thumbnails, search, zoom, and a restored
reading position. Conversations are kept alongside the document, so the questions you
asked last week are still there.

## A typical session

1. Open a PDF and index it in the background.
2. Drag over a step you do not follow and ask "why does this hold?".
3. Read the short explanation, ask for more if you need it, then confirm it.
4. Highlight the passage that finally makes sense; add a note in your own words.
5. Close the app. Next time, the page, the highlights and the conversation are where
   you left them.

## Install (Windows 10/11, 64-bit)

1. Download `AI-Native PDF Reader_1.0.3_x64-setup.exe` and check its SHA-256 against
   the published value.
2. Run it. The installer is unsigned, so Windows may warn about an unknown publisher.
3. Launch the app and open a PDF.

WebView2 is required; Windows 11 has it, and on Windows 10 you may need Microsoft's
runtime first. The installer does not bundle one.

## Connect your own model

The app has no key of its own and never calls a model in the background.

1. Open **Settings → Model**.
2. Choose DeepSeek, OpenRouter, or any OpenAI-compatible endpoint, and set the model.
3. Paste your API key. It is stored in the local credential store only — never in the
   front end, never in logs, never in exports.
4. Use **Test connection**. Without a key you can still read and annotate, and the Mock
   model lets you try the interface.

## Your data

- Settings: `%APPDATA%\io.github.ainativepdfreader\`
- Documents, index, conversations, backups:
  `%LOCALAPPDATA%\io.github.ainativepdfreader\data-manager\`

Everything above stays on this machine. When you ask a question, the selected passage
and the pages you chose go to the endpoint you configured, and nothing else.

## Honest limitations

- Windows only, and the installer is unsigned.
- PDFs close to 16 MB are refused at save time; the original and your draft are kept.
- Annotations display correctly in other readers, but full edit round-trips in Edge or
  Adobe are not claimed.
- Scanned pages without a text layer are treated as images, not searchable text.
- AI answers can be wrong. The app cites the source so you can check it; treat it as a
  study aid, not an authority.

## Build from source

Requires Node.js 20+, Rust 1.98+ with the MSVC toolchain, and WebView2.

```powershell
npm ci                  # dependencies
npm run prepare:assets  # copy PDF.js/MuPDF assets, fetch the pinned font, verify SHA-256
npm run build           # frontend bundle
npm run desktop:build   # desktop binary
```

`npm run prepare:assets` downloads the pinned Noto Sans SC face once and checks both
its size and SHA-256 against `worker/freetext-font.json`; a mismatch is refused. The
licence text ships in `worker/fonts/`.

Some tests need fixtures that are not shipped (sample PDFs and a local worker runtime);
the build itself does not.

## Licence

**AGPL-3.0-or-later.** See [LICENSE](LICENSE). The app ships MuPDF.js, which is
AGPL-3.0-or-later, so the distributed work as a whole is licensed the same way. If you
run a modified version as a network service, the AGPL requires you to offer its source
to those users.

Third-party components and their licences are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
