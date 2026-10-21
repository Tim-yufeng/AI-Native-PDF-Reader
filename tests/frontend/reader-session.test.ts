import { test } from 'node:test';
import assert from 'node:assert/strict';
import { setImmediate } from 'node:timers/promises';
import { ReaderSession } from '../../src/lib/reader-session.ts';
import type { ReaderState, LoadingTask } from '../../src/lib/reader-session.ts';
import { browserFileSource, MAX_INPUT_BYTES } from '../../src/lib/document-source.ts';
import type { PdfSource } from '../../src/lib/document-source.ts';

type Document = { numPages: number; id: number };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function harness() {
  const states: ReaderState[] = [];
  const documents: (Document | null)[] = [];
  const tasks: (LoadingTask<Document> & ReturnType<typeof deferred<Document>> & { destroyed: number })[] = [];
  const session = new ReaderSession<Document>(() => {
    const pending = deferred<Document>();
    const task = { ...pending, destroyed: 0, async destroy() { this.destroyed++; } };
    tasks.push(task); return task;
  }, state => states.push(state), document => documents.push(document));
  return { session, tasks, states, documents, state: () => states.at(-1)! };
}
const source = (id = 1): PdfSource => ({ name: `中文 示例 ${id}.PDF`, size: 1,
  async read(signal) { signal.throwIfAborted(); return new Uint8Array([id]); } });

test('opens, closes and reopens the same file; destroys each document once', async () => {
  const h = harness();
  for (let round = 0; round < 2; round++) {
    const opening = h.session.open(source()); await setImmediate();
    h.tasks[round].resolve({ id: round, numPages: 2 }); await opening;
    assert.equal(h.state().status, 'ready'); assert.equal(h.state().pageCount, 2);
    assert.equal(h.state().name, '中文 示例 1.PDF');
    await h.session.close(); await h.session.close();
    assert.equal(h.state().status, 'idle'); assert.equal(h.tasks[round].destroyed, 1);
  }
});

for (const [name, size] of [['not-pdf.txt', 1], ['empty.pdf', 0], ['large.pdf', MAX_INPUT_BYTES + 1]] as const) {
  test(`rejects ${name} before reading bytes or starting the parser`, async () => {
    const h = harness(); let read = false;
    await h.session.open({ name, size, async read() { read = true; return new Uint8Array(1); } });
    assert.equal(h.state().status, 'error'); assert.equal(read, false); assert.equal(h.tasks.length, 0);
  });
}
test('rejects incomplete or changed byte reads', async () => {
  const h = harness(); await h.session.open({ ...source(), size: 2 });
  assert.equal(h.state().status, 'error'); assert.match(h.state().error, /大小不一致/);
  assert.equal(h.tasks.length, 0);
});
test('a late file read cannot overwrite a newer document', async () => {
  const h = harness(); const oldRead = deferred<Uint8Array>(); let signal!: AbortSignal;
  const old = h.session.open({ ...source(1), read(abort) { signal = abort; return oldRead.promise; } });
  await setImmediate(); const current = h.session.open(source(2)); await setImmediate();
  assert.equal(signal.aborted, true);
  h.tasks[0].resolve({ id: 2, numPages: 1 }); await current;
  oldRead.resolve(new Uint8Array([1])); await old;
  assert.equal(h.tasks.length, 1); assert.equal(h.documents.at(-1)?.id, 2);
  assert.equal(h.state().name, source(2).name);
});
test('a late parser completion cannot attach after replacement', async () => {
  const h = harness(); const old = h.session.open(source(1)); await setImmediate();
  const current = h.session.open(source(2)); await setImmediate();
  assert.equal(h.tasks[0].destroyed, 1);
  h.tasks[1].resolve({ id: 2, numPages: 1 }); await current;
  h.tasks[0].resolve({ id: 1, numPages: 3 }); await old;
  assert.equal(h.documents.at(-1)?.id, 2); assert.equal(h.tasks[0].destroyed, 1);
});
test('close and component disposal block late parser results and subsequent opens', async () => {
  const h = harness(); const opening = h.session.open(source()); await setImmediate();
  await h.session.close(); const stateCount = h.states.length;
  h.tasks[0].resolve({ id: 1, numPages: 1 }); await opening;
  assert.equal(h.state().status, 'idle'); assert.equal(h.states.length, stateCount);
  assert.equal(h.documents.at(-1), null); assert.equal(h.tasks[0].destroyed, 1);
  await h.session.dispose(); await h.session.open(source(2));
  assert.equal(h.tasks.length, 1);
});
test('parser error is recoverable and its raw details are not displayed', async () => {
  const h = harness(); const broken = h.session.open(source()); await setImmediate();
  h.tasks[0].reject(Object.assign(new Error('private parser detail'), { name: 'InvalidPDFException' }));
  await broken; assert.equal(h.state().status, 'error'); assert.match(h.state().error, /损坏/);
  assert.doesNotMatch(h.state().error, /private/); assert.equal(h.tasks[0].destroyed, 1);
  const retry = h.session.open(source(2)); await setImmediate();
  h.tasks[1].resolve({ id: 2, numPages: 1 }); await retry;
  assert.equal(h.state().status, 'ready'); assert.equal(h.state().error, '');
});
test('password-protected documents receive an explicit deferred-feature message', async () => {
  const h = harness(); const opening = h.session.open(source()); await setImmediate();
  h.tasks[0].reject(Object.assign(new Error('password'), { name: 'PasswordException' }));
  await opening; assert.match(h.state().error, /需要密码/);
});
test('a late cleanup failure cannot clear the newer active document', async () => {
  const h = harness(); const cleanup = deferred<void>();
  const first = h.session.open(source(1)); await setImmediate();
  h.tasks[0].resolve({ id: 1, numPages: 1 }); await first;
  h.tasks[0].destroy = () => cleanup.promise;
  const closing = h.session.close();
  const second = h.session.open(source(2)); await setImmediate();
  h.tasks[1].resolve({ id: 2, numPages: 1 }); await second;
  cleanup.reject(new Error('Old cleanup failed')); await closing;
  assert.equal(h.state().status, 'ready'); assert.equal(h.state().name, source(2).name);
  assert.equal(h.documents.at(-1)?.id, 2); assert.equal(h.state().error, '');
});
test('invalid page counts are rejected and their parser is released', async () => {
  for (const numPages of [0, 2001, NaN]) {
    const h = harness(); const opening = h.session.open(source()); await setImmediate();
    h.tasks[0].resolve({ id: 1, numPages }); await opening;
    assert.equal(h.state().status, 'error'); assert.equal(h.tasks[0].destroyed, 1);
  }
});
test('browser source returns read-only bytes and honors cancellation', async () => {
  const file = new File([new Uint8Array([1, 2, 3])], '中文 空格.pdf');
  const input = browserFileSource(file); const abort = new AbortController();
  assert.deepEqual(await input.read(abort.signal), new Uint8Array([1, 2, 3]));
  abort.abort(); await assert.rejects(input.read(abort.signal), { name: 'AbortError' });
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), new Uint8Array([1, 2, 3]));
});
test('twenty real parser open-close cycles leave no attached document or live loading task', async () => {
  const { getDocument } = await import('pdfjs-dist/legacy/build/pdf.mjs');
  const { readingFixture } = await import('./reading-fixture.ts');
  let attached: import('pdfjs-dist').PDFDocumentProxy | null = null, destroyed = 0, created = 0;
  const session = new ReaderSession(bytes => {
    const task = getDocument({ data: bytes }); created++;
    return { promise: task.promise, async destroy() { await task.destroy(); destroyed++; } };
  }, () => {}, document => { attached = document; });
  for (let round=0;round<20;round++) {
    const bytes=readingFixture(2); await session.open({ name:'cycle.pdf',size:bytes.length,read:async()=>bytes });
    assert.ok(attached); await session.close(); assert.equal(attached,null); assert.equal(created,destroyed);
  }
  await session.dispose(); assert.equal(created,20); assert.equal(destroyed,20);
});
