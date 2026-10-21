import test from 'node:test';
import assert from 'node:assert/strict';
import { desktopError, desktopSource, writerError, type DesktopBridge } from '../../src/lib/desktop-source.ts';
import { SourceError } from '../../src/lib/document-source.ts';
import { WriterRefusal, writerRefusalMessages, writerSteps, type WriterRefusalCode, type WriterStep } from '../../src/lib/writer-refusal.ts';

const fakePrivate = 'FAKE-SECRET-NOT-A-KEY C:\\synthetic-private\\data.db SELECT private_content';
const descriptor = { grantId: '9b6dfae0-3f79-4ee9-9a12-6d8de0b2f2f2', name: '合成验证.pdf', size: 5 };
const codes = Object.keys(writerRefusalMessages) as WriterRefusalCode[];
const steps = Object.keys(writerSteps) as WriterStep[];

test('every writer refusal reports its approved code and step, never the unclassified fallback', () => {
  for (const code of codes) {
    for (const step of steps) {
      const display = writerError(new WriterRefusal(code, step), 'serialize');
      assert.match(display, /生成 PDF 批注未完成/);
      assert.ok(display.includes(code), `${code} must be reported`);
      assert.ok(display.includes(writerRefusalMessages[code]), `${code} must show only its approved text`);
      assert.ok(display.includes(`步骤：${writerSteps[step]}`), `${code}/${step} must report the failing step`);
      assert.ok(!display.includes('原因未能安全识别'));
      assert.equal(desktopError(new WriterRefusal(code, step)), writerRefusalMessages[code]);
    }
  }
});

test('the writer vocabulary is closed: forged or inherited fields are ignored', () => {
  const forged = { code: 'writer_glyph_missing', writerStep: fakePrivate, message: fakePrivate };
  const display = writerError(forged, 'serialize');
  assert.match(display, /writer_glyph_missing/);
  assert.ok(!display.includes('步骤：'));
  assert.ok(!display.includes(fakePrivate));
  const inherited = Object.assign(Object.create({ writerStep: 'appearance' }), { code: 'writer_glyph_missing' });
  assert.ok(!writerError(inherited, 'serialize').includes('步骤：'));
  let touched = false;
  const getter = Object.defineProperty({ code: 'writer_glyph_missing' }, 'writerStep', { get() { touched = true; throw Error(fakePrivate); } });
  assert.ok(!writerError(getter, 'serialize').includes('步骤：'));
  const hostile = new Proxy({ code: 'writer_subset_failed' }, { getOwnPropertyDescriptor(target, key) { if (key === 'code') return Object.getOwnPropertyDescriptor(target, key); throw Error(fakePrivate); } });
  assert.match(writerError(hostile, 'serialize'), /writer_subset_failed/);
  assert.equal(touched, false);
});

test('a refusal keeps its code, step and approved message immutable and never leaks the engine cause', () => {
  const refusal = new WriterRefusal('writer_engine_unavailable', 'engine', new Error(fakePrivate));
  assert.ok(refusal instanceof Error);
  assert.equal(refusal.message, writerRefusalMessages.writer_engine_unavailable);
  assert.equal(Object.getOwnPropertyDescriptor(refusal, 'code')?.writable, false);
  assert.equal(Object.getOwnPropertyDescriptor(refusal, 'writerStep')?.writable, false);
  const display = writerError(refusal, 'finish');
  assert.match(display, /MuPDF.js 引擎初始化失败/);
  assert.ok(!display.includes(fakePrivate));
  assert.ok(!JSON.stringify(refusal).includes(fakePrivate));
});

test('the step survives the export bridge wrapping that turns a refusal into a source error', async () => {
  const refusal = new WriterRefusal('writer_no_annotation', 'serialize');
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    if (command === 'begin_unicode_export') return 'synthetic-task' as T;
    return undefined as T;
  };
  const writeCandidate = async () => { throw refusal; };
  let caught: unknown;
  try {
    await desktopSource(descriptor, bridge, writeCandidate).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal);
  } catch (error) { caught = error; }
  assert.ok(caught instanceof SourceError);
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStage')?.value, 'serialize');
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStep')?.value, 'serialize');
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStep')?.writable, false);
  const display = writerError(caught, 'finish');
  assert.match(display, /writer_no_annotation/);
  assert.match(display, /步骤：批注序列化/);
  assert.ok(!display.includes('原因未能安全识别'));
});

test('a font fetch refusal reports a bounded HTTP status and nothing else', () => {
  const display = writerError(new WriterRefusal('writer_font_fetch_failed', 'resources', new Error(fakePrivate), 404), 'serialize');
  assert.match(display, /writer_font_fetch_failed/);
  assert.match(display, /步骤：资源校验；HTTP 状态：404。/);
  assert.ok(!display.includes(fakePrivate));
  for (const value of [-1, 600, 404.5, NaN, Infinity, '404', null, fakePrivate]) {
    const bounded = writerError(new WriterRefusal('writer_font_fetch_failed', 'resources', undefined, value as number), 'serialize');
    assert.ok(!bounded.includes('HTTP 状态：'), `status ${String(value)} must not be reported`);
    assert.ok(!bounded.includes(fakePrivate));
  }
  assert.ok(!writerError({ code: 'writer_font_fetch_failed', httpStatus: 999 }, 'serialize').includes('HTTP 状态：'));
  const inherited = Object.assign(Object.create({ httpStatus: 404 }), { code: 'writer_font_fetch_failed' });
  assert.ok(!writerError(inherited, 'serialize').includes('HTTP 状态：'));
  let touched = false;
  const getter = Object.defineProperty({ code: 'writer_font_fetch_failed' }, 'httpStatus', { get() { touched = true; throw Error(fakePrivate); } });
  assert.ok(!writerError(getter, 'serialize').includes('HTTP 状态：'));
  assert.equal(touched, false);
  assert.equal(Object.getOwnPropertyDescriptor(new WriterRefusal('writer_font_fetch_failed', 'resources', undefined, 404), 'httpStatus')?.writable, false);
});

test('a font fetch refusal survives the export wrapping with its step and status', async () => {
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => command === 'begin_unicode_export' ? 'synthetic-task' as T : undefined as T;
  const writeCandidate = async () => { throw new WriterRefusal('writer_font_fetch_failed', 'resources', new Error(fakePrivate), 403); };
  let caught: unknown;
  try {
    await desktopSource(descriptor, bridge, writeCandidate).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal);
  } catch (error) { caught = error; }
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStep')?.value, 'resources');
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'httpStatus')?.value, 403);
  const display = writerError(caught, 'finish');
  assert.match(display, /步骤：资源校验；HTTP 状态：403。/);
  assert.ok(!display.includes('原因未能安全识别'));
});

test('a changed or replaced original is reported with its own code and next step', () => {
  const display = writerError({ code: 'writer_source_changed' }, 'finish');
  assert.match(display, /写入批注副本未完成（writer_source_changed）/);
  assert.match(display, /请重新打开该文件后再保存/);
  assert.ok(!display.includes('未取得错误码'));
});

test('an unclassified rejection does not claim whether saving changed a file', () => {
  // This is the shape a capability or transport rejection has: no approved code.
  const display = writerError('save_original_pdf not allowed', 'finish');
  assert.match(display, /写入批注副本未完成/);
  assert.match(display, /未取得错误码，无法确认失败阶段与保存结果/);
  assert.ok(!display.includes('not allowed'));
  assert.ok(!display.includes('原因未能安全识别'));
});

test('the saved-copy path reports a native writer stage without inventing a step', async () => {
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    if (command === 'begin_unicode_export') throw { code: 'writer_isolation_blocked', message: fakePrivate };
    return undefined as T;
  };
  let caught: unknown;
  try {
    await desktopSource(descriptor, bridge).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal);
  } catch (error) { caught = error; }
  const display = writerError(caught, 'finish');
  assert.match(display, /准备另存为未完成/);
  assert.match(display, /writer_isolation_blocked/);
  assert.ok(!display.includes('步骤：'));
  assert.ok(!display.includes(fakePrivate));
});
