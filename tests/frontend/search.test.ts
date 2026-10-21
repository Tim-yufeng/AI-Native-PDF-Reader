import test from 'node:test';
import assert from 'node:assert/strict';
import { getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { readingFixture } from './reading-fixture.ts';
import { resolve } from 'node:path';
// Only bootstrap style/animation globals for the real search controller, not a GUI simulator.
Object.assign(globalThis, { document: { documentElement: { style: {} }, createElement() { return { style: {} }; } }, window: { requestAnimationFrame(callback: (time: number) => void) { callback(0); } } });
const { EventBus, PDFFindController, PDFLinkService, FindState } = await import('pdfjs-dist/web/pdf_viewer.mjs');
test('locked search controller finds all pages, advances, reverses and resets for a scan', async () => {
  const events = new EventBus(), links = new PDFLinkService({ eventBus: events });
  const finder = new PDFFindController({ eventBus: events, linkService: links, delay: 0 });
  finder.onIsPageVisible = () => true;
  const text = getDocument({ data: readingFixture(), standardFontDataUrl: resolve('node_modules/pdfjs-dist/standard_fonts') + '/' });
  const scan = getDocument({ data: readingFixture(2, true) });
  let page = 1;
  links.setViewer({ get currentPageNumber() { return page; }, set currentPageNumber(value: number) { page = value; }, pagesRotation: 0 } as unknown as Parameters<typeof links.setViewer>[0]);
  type Result = { state?: number; matchesCount: { current: number; total: number } };
  function result(query: string, expected: (event: Result) => boolean, previous = false, again = false): Promise<Result> {
    return new Promise((done, reject) => {
      const callback = (event: Result) => { if (expected(event)) { clearTimeout(timer); events.off('updatefindcontrolstate', callback); events.off('updatefindmatchescount', callback); done(event); } };
      const timer = setTimeout(() => { events.off('updatefindcontrolstate', callback); events.off('updatefindmatchescount', callback); reject(Error('Search timed out')); }, 5000);
      events.on('updatefindcontrolstate', callback); events.on('updatefindmatchescount', callback);
      events.dispatch('find', { query, type: again ? 'again' : '', highlightAll: true, caseSensitive: false, entireWord: false, findPrevious: previous, matchDiacritics: false });
    });
  }
  try {
    const pdf = await text.promise; links.setDocument(pdf); finder.setDocument(pdf);
    const all = await result('needle', event => event.matchesCount.total === 240); assert.equal(all.matchesCount.current, 1);
    await result('needle', event => event.matchesCount.current === 2, false, true);
    await result('needle', event => event.matchesCount.current === 1, true, true);
    await result('Reading page 90', event => event.matchesCount.total === 1); assert.equal(page, 90);
    const image = await scan.promise; links.setDocument(image); finder.setDocument(image); page = 1;
    const none = await result('needle', event => event.state === FindState.NOT_FOUND); assert.equal(none.matchesCount.total, 0);
  } finally { finder.setDocument(null as unknown as Parameters<typeof finder.setDocument>[0]); await text.destroy(); await scan.destroy(); }
});

