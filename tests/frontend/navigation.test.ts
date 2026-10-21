import { test } from 'node:test';
import assert from 'node:assert/strict';
import { setImmediate } from 'node:timers/promises';
import { pageNumber, thumbnailWindow } from '../../src/lib/navigation.ts';
import { ThumbnailQueue } from '../../src/lib/thumbnail-queue.ts';
import { readingFixture } from './reading-fixture.ts';
import { getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { resolve } from 'node:path';

test('page input accepts valid integers and refuses invalid or out-of-range values', () => {
  assert.equal(pageNumber(' 27 ', 120), 27); assert.equal(pageNumber('001', 120), 1);
  for (const value of ['', '0', '-1', '2.5', '1e2', 'Infinity', '121', '99999999999999999']) {
    assert.equal(pageNumber(value, 120), null);
  }
});
test('thumbnail viewport stays bounded for 2000 pages and includes nearby rows', () => {
  assert.deepEqual(thumbnailWindow(0, 360, 2000), [1, 2, 3, 4]);
  const middle = thumbnailWindow(90000, 600, 2000);
  assert.ok(middle.includes(501)); assert.ok(middle.length <= 9);
  const last = thumbnailWindow(359640, 360, 2000);
  assert.equal(last.at(-1), 2000); assert.ok(last.length <= 6);
});
test('only two thumbnails run concurrently; canceled queued work never starts', async () => {
  const queue = new ThumbnailQueue(); let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  const controllers = Array.from({ length: 4 }, () => new AbortController());
  const started: number[] = [];
  const jobs = controllers.map((controller, index) => queue.schedule(async () => {
    started.push(index); await pending;
  }, controller.signal));
  const canceled = assert.rejects(jobs[2], { name: 'AbortError' });
  await setImmediate(); assert.equal(queue.active, 2); assert.equal(queue.queued, 2);
  controllers[2].abort(); await canceled; assert.equal(queue.queued, 1);
  release(); await Promise.all([jobs[0], jobs[1], jobs[3]]); await setImmediate();
  assert.deepEqual(started, [0, 1, 3]); assert.equal(queue.active, 0); assert.equal(queue.queued, 0);
});
test('new synthetic multi-page and image-only controls parse correctly', async () => {
  for (const imageOnly of [false, true]) {
    const task = getDocument({ data: readingFixture(imageOnly ? 2 : 120, imageOnly), standardFontDataUrl: resolve('node_modules/pdfjs-dist/standard_fonts') + '/' });
    try {
      const document = await task.promise; assert.equal(document.numPages, imageOnly ? 2 : 120);
      const text = await (await document.getPage(1)).getTextContent();
      assert.equal(text.items.length === 0, imageOnly);
      if (!imageOnly) assert.equal((await document.getPage(7)).rotate, 90);
    } finally { await task.destroy(); }
  }
});
