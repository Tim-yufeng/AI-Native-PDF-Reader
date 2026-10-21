# AI-Native PDF Reader

English | [简体中文](README.zh-CN.md)

You're reading a PDF and get stuck on something. What do you do? Open an AI chat,
take a screenshot or upload the whole PDF, then wait for a reply? Switching back and
forth is a hassle. What if you could ask AI for help right inside the PDF reader?

That's what AI-Native PDF Reader is for: select a passage, formula or diagram, ask
your question, and place the explanation beside the original text. You can also
highlight, draw and take notes as usual.

The current release is **1.0.3 Public Beta**. You can read, annotate and save PDFs
without a model connection. Online AI features require your own API key.

## Features

- Tabs, thumbnails, document outline, search, zoom and restored reading position.
- Highlights, freehand ink and text boxes, saved into the PDF with `Ctrl+S`.
- Questions about selected text, a dragged region or multiple pages, with optional
  retrieval from the current PDF.
- Temporary explanations beside the passage. Confirm one to turn it into a text
  annotation, then press `Ctrl+S` to save it. You can also regenerate or dismiss it.
- Follow-up questions in the side chat, with history stored locally and available
  when you reopen the document.
- Custom explanation styles you can create, import and switch, plus configurable
  in-app shortcuts.

## Download and install

Windows 10/11, 64-bit.

1. Download `AI-Native PDF Reader_1.0.3_x64-setup.exe` from the release page and
   compare its SHA-256 with the published value.
2. Run the installer. It is unsigned, so Windows may show an unknown publisher warning.
3. Launch the app and open a PDF.

WebView2 is required and is not bundled with the installer. If it is missing from
your system, install Microsoft's runtime first.

## Set up AI

Open **Settings → Model**, choose DeepSeek, OpenRouter or a custom compatible
endpoint, and enter the model name and your API key. Select **Save configuration**.
Custom endpoints currently support public HTTPS Chat Completions APIs.

API keys are stored in the Windows credential store. To try the interface without
an online service, use the Mock model.

**Test connection** checks the model catalogue connection. To check how a model
actually answers, you still need to ask it a question.

### Using the main and auxiliary models

In model settings, use **Model role** to configure the main and auxiliary models
separately. Enable the auxiliary model if you want to use one.

- **Main model:** a more capable model is useful for detailed analysis, working
  through derivations and follow-up questions in the side chat.
- **Auxiliary model:** a lower-cost model can handle short questions about terms
  or symbols, or generate temporary explanations.

Once configured, choose the model for each request using the picker in the side chat
or the in-place question window. Each remembers its own selection. If an in-place
box is only showing the answer, click the edit button to open the question window
and switch models.

My recommendation: use a cheaper auxiliary model API, or one with a free allowance,
for temporary explanations, and keep the main model for questions that need deeper
analysis. For screenshots or diagrams, choose a model that supports images and
enable **Supports images** in model settings.

## Data storage

Default locations:

- Settings: `%APPDATA%\io.github.ainativepdfreader\`
- App data, including the index, conversations, recovery drafts and backups:
  `%LOCALAPPDATA%\io.github.ainativepdfreader\data-manager\`

Saved annotations live in the PDF itself. Conversation history is stored separately
on your machine.

When you send an AI question, the app sends it and the context for that request to
your configured model service. Context may include selected text or images, pages
you chose or the app retrieved, relevant conversation history and explanation style
instructions. For questions in the side chat, you can review and remove sources in
the context options. Retrieval from other PDFs in the same folder requires separate
permission.

## Known limitations

- Windows only; the installer is unsigned.
- Saving has file size and structure limits. PDFs close to 16 MB may be refused;
  the original and recovery draft are kept when a limit is reached.
- Annotations are saved as standard PDF annotations, but display and further editing
  in Edge and Adobe still need more verification.
- Scanned pages without a text layer are treated as images and cannot be searched
  as text.
- AI explanations can be wrong. Check the cited sources, especially for formulas,
  derivations and unstated assumptions.

## Build from source

Requires Node.js 24.19.0+ and npm 11+. The Windows desktop build also needs Rust
1.98+, the MSVC toolchain and WebView2.

```powershell
npm ci
npm run build           # frontend
npm run desktop:build   # desktop executable, without an installer
```

The build prepares PDF.js and MuPDF.js assets. If the font is missing locally, it
downloads a pinned Noto Sans SC font and checks its size and SHA-256 against
`worker/freetext-font.json`. The font licence is included in the source.

The default desktop build uses Mock and does not enable online models. The build
script, `scripts/desktop.mjs`, still uses the development machine's Rust path and
toolchain version; adjust these before building on another machine. Some tests also
require sample PDFs and a local worker runtime that are not included.

## Licence

**AGPL-3.0-or-later.** See [LICENSE](LICENSE) for the full text. The app includes
MuPDF.js; third-party components and their licences are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
