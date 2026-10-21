import test from 'node:test';
import assert from 'node:assert/strict';
import { desktopError, desktopSource, writerError, type DesktopBridge, type WriterStage } from '../../src/lib/desktop-source.ts';
import { SourceError } from '../../src/lib/document-source.ts';

const fakePrivate = 'FAKE-SECRET-NOT-A-KEY C:\\synthetic-private\\data.db SELECT private_content';
const descriptor = { grantId: '3ee1a7ee-82db-4554-8d84-751dd97b8880', name: '合成验证.pdf', size: 5 };
test('writer diagnostics preserve native public code and operation without trusting backend message', () => {
  const error = { code: 'writer_startup_2', message: fakePrivate, stack: fakePrivate, path: fakePrivate };
  const display = writerError(error, 'check');
  assert.match(display, /Writer 隔离检查未完成/);
  assert.match(display, /writer_startup_2/);
  assert.match(display, /Windows 2/);
  assert.ok(!display.includes(fakePrivate));
  const stages: WriterStage[] = ['check', 'serialize', 'begin', 'finish', 'reopen'];
  assert.equal(new Set(stages.map(stage => writerError({ code: 'writer_export_failed', message: fakePrivate }, stage))).size, 5);
});
test('normal Error exposes only exact preapproved static text or an approved own code', () => {
  const approved = desktopError({ code: 'writer_target_exists' });
  assert.match(writerError(new Error(approved), 'finish'), /writer_target_exists/);
  assert.match(writerError(new SourceError(approved), 'finish'), /writer_target_exists/);
  const withCode = Object.assign(new Error(fakePrivate), { code: 'writer_probe_failed' });
  assert.match(writerError(withCode, 'check'), /writer_probe_failed/);
  for (const error of [new Error(fakePrivate), new SourceError(fakePrivate), new Error(approved + fakePrivate)]) {
    const display = writerError(error, 'serialize');
    assert.match(display, /生成 PDF 批注未完成/);
    // A missing code does not prove the backend was never called.
    assert.match(display, /未取得错误码，无法确认失败阶段与保存结果/);
    assert.ok(!display.includes(fakePrivate));
  }
});
test('unknown strings, object fields and hostile property access never leak or execute getters', () => {
  let touched = false;
  const getter = Object.defineProperty({}, 'code', { get() { touched = true; throw Error(fakePrivate); } });
  const messageGetter = Object.defineProperty({}, 'message', { get() { touched = true; return fakePrivate; } });
  const inherited = Object.create({ code: 'writer_probe_failed', message: fakePrivate });
  const hostile = new Proxy({}, { getOwnPropertyDescriptor() { throw Error(fakePrivate); } });
  const revoked = Proxy.revocable({}, {}); revoked.revoke();
  const stringify = { code: { toString() { touched = true; return 'writer_probe_failed'; } }, message: fakePrivate };
  for (const error of [fakePrivate, null, undefined, 42, [fakePrivate], { code: fakePrivate, message: fakePrivate },
    { code: 'constructor', message: fakePrivate }, { code: '__proto__', message: fakePrivate },
    { code: 'unknown', message: desktopError({ code: 'writer_probe_failed' }) }, getter, messageGetter, inherited, hostile, revoked.proxy, stringify]) {
    const display = writerError(error, 'check');
    assert.equal(typeof display, 'string');
    assert.match(display, /未取得错误码/);
    assert.ok(!display.includes(fakePrivate));
    assert.ok(!desktopError(error).includes(fakePrivate));
  }
  assert.equal(touched, false);
});
test('export bridge preserves known rejection codes through compatible SourceError wrapping', async () => {
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    if (command === 'begin_unicode_export') return 'synthetic-task' as T;
    if (command === 'finish_unicode_export') throw { code: 'writer_conversion_refused', message: fakePrivate };
    return undefined as T;
  };
  // A stub writer keeps these bridge-code tests deterministic; the real MuPDF
  // writer is covered by the spike matrix and the Rust writer gates.
  const stubWriter = async (candidate: Uint8Array) => new Uint8Array([...candidate, 10, 37, 37, 69, 79, 70]);
  let caught: unknown;
  try { await desktopSource(descriptor, bridge, stubWriter).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal); }
  catch (error) { caught = error; }
  assert.ok(caught instanceof SourceError);
  const display = writerError(caught, 'finish');
  assert.match(display, /writer_conversion_refused/);
  assert.ok(!display.includes(fakePrivate));
  assert.match(display, /写入批注副本未完成/);
  assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStage')?.value, 'finish');
});
test('begin rejection keeps its real phase despite caller finish default and preserves known SourceError text', async () => {
  for (const error of [{ code: 'file_changed', message: fakePrivate }, new SourceError(desktopError({ code: 'writer_isolation_blocked' }))]) {
    const calls: string[] = [];
    const bridge: DesktopBridge = async <T>(command: string): Promise<T> => { calls.push(command); throw error; };
    let caught: unknown;
    try { await desktopSource(descriptor, bridge).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal); }
    catch (value) { caught = value; }
    assert.ok(caught instanceof SourceError);
    assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStage')?.value, 'begin');
    assert.equal(Object.getOwnPropertyDescriptor(caught, 'writerStage')?.writable, false);
    const display = writerError(caught, 'finish');
    assert.match(display, /准备另存为未完成/);
    assert.match(display, /file_changed|writer_isolation_blocked/);
    assert.ok(!display.includes(fakePrivate));
    assert.deepEqual(calls, ['begin_unicode_export']);
  }
});
test('writer stage override accepts known own data only and never evaluates getters or unknown text', () => {
  let touched = false;
  const getter = Object.defineProperty({ code: 'writer_probe_failed' }, 'writerStage', { get() { touched = true; throw Error(fakePrivate); } });
  assert.match(writerError(getter, 'finish'), /写入批注副本未完成/);
  for (const value of [fakePrivate, 'constructor', '__proto__', { toString() { touched = true; return 'begin'; } }]) {
    const display = writerError({ code: 'writer_probe_failed', writerStage: value }, 'finish');
    assert.match(display, /写入批注副本未完成/);
    assert.ok(!display.includes(fakePrivate));
  }
  const inherited = Object.create({ writerStage: 'begin' }); inherited.code = 'writer_probe_failed';
  assert.match(writerError(inherited, 'finish'), /写入批注副本未完成/);
  assert.match(writerError({ code: 'writer_probe_failed', writerStage: 'check' }, 'finish'), /Writer 隔离检查未完成/);
  assert.equal(touched, false);
});

test('profile diagnostics display only approved stages, reasons and bounded Windows error numbers', () => {
  for (const code of ['writer_profile_rejected', 'writer_profile_create_failed']) {
    const display = writerError({ code, message: fakePrivate, sandboxStage: 'profile-ac', sandboxReason: 'incomplete-dacl', windowsError: 5 }, 'check');
    assert.match(display, /检查位置：隔离工作目录权限/);
    assert.match(display, /原因：所需权限不完整/);
    assert.match(display, /Windows 错误：5（0x00000005）/);
    assert.ok(!display.includes(fakePrivate));
  }
  assert.match(writerError({ code: 'writer_profile_rejected', windowsError: 0 }, 'check'), /0（0x00000000）/);
  assert.match(writerError({ code: 'writer_profile_rejected', windowsError: 0xffffffff }, 'check'), /4294967295（0xffffffff）/);
  assert.match(writerError({ code: 'writer_profile_rejected', sandboxStage: 'identity', sandboxReason: 'unmarked-profile' }, 'check'), /缺少本应用的有效标记/);
  const unrelated = writerError({ code: 'writer_export_failed', sandboxStage: 'identity', sandboxReason: 'owner', windowsError: 5 }, 'check');
  assert.ok(!unrelated.includes('检查位置：'));
  assert.ok(!unrelated.includes('Windows 错误：'));
});

test('probe failures identify the failed check and distinguish process exit from Windows API failure', () => {
  const network = writerError({ code: 'writer_probe_failed', sandboxStage: 'probe-network', sandboxReason: 'denial-unproven', message: fakePrivate }, 'check');
  assert.match(network, /网络隔离证明/); assert.match(network, /未能证明/); assert.ok(!network.includes(fakePrivate));
  const job = writerError({ code: 'writer_probe_failed', sandboxStage: 'worker-job-assignment', sandboxReason: 'access-denied', windowsError: 5 }, 'check');
  assert.match(job, /进程资源限制绑定/); assert.match(job, /Windows 错误：5/);
  const environment = writerError({ code: 'writer_probe_failed', sandboxStage: 'worker-start', sandboxReason: 'environment-missing', windowsError: 203 }, 'check');
  assert.match(environment, /Windows 隔离启动所需环境信息缺失/); assert.match(environment, /203（0x000000cb）/);
  const access = writerError({ code: 'writer_probe_failed', sandboxStage: 'worker-token-access', sandboxReason: 'isolation-mismatch', message: fakePrivate }, 'check');
  assert.match(access, /核对实际隔离授权/); assert.match(access, /未满足完整隔离对照/); assert.ok(!access.includes(fakePrivate));
  for (const sandboxStage of ['probe-appcontainer-control', 'probe-lpac-control']) {
    assert.match(writerError({ code: 'writer_probe_failed', sandboxStage }, 'check'), /隔离进程对照/);
  }
  const entry = writerError({ code: 'writer_probe_failed', sandboxStage: 'entry-python-load', sandboxReason: 'access-denied', windowsError: 5, workerExit: 125 }, 'check');
  assert.match(entry, /载入 Python 核心及其依赖/); assert.match(entry, /Windows 错误：5/); assert.match(entry, /进程退出码：125/);
  const exit = writerError({ code: 'writer_probe_failed', sandboxStage: 'worker-exit', sandboxReason: 'child-exited', workerExit: 0xc0000022 }, 'check');
  assert.match(exit, /进程退出码：3221225506（0xc0000022）/); assert.ok(!exit.includes('Windows 错误：'));
  for (const value of [-1, Infinity, NaN, 0x100000000, 2.5, '5', fakePrivate]) {
    assert.ok(!writerError({ code: 'writer_probe_failed', workerExit: value }, 'check').includes('进程退出码：'));
  }
  let touched = false;
  const getter = Object.defineProperty({ code: 'writer_probe_failed' }, 'workerExit', { get() { touched = true; throw Error(fakePrivate); } });
  assert.ok(!writerError(getter, 'check').includes('进程退出码：')); assert.equal(touched, false);
  const inherited = Object.assign(Object.create({ workerExit: 0xc0000022 }), { code: 'writer_probe_failed' });
  assert.ok(!writerError(inherited, 'check').includes('进程退出码：'));
});

test('sandbox diagnostics ignore inherited, hostile and arbitrary fields without reading raw data', () => {
  let touched = false;
  const getter = { code: 'writer_profile_rejected' };
  for (const key of ['sandboxStage', 'sandboxReason', 'windowsError', 'message', 'path', 'sid']) {
    Object.defineProperty(getter, key, { get() { touched = true; throw Error(fakePrivate); } });
  }
  Object.defineProperty(getter, 'toString', { value() { touched = true; throw Error(fakePrivate); } });
  const inherited = Object.assign(Object.create({ sandboxStage: 'profile-ac', sandboxReason: 'owner', windowsError: 5 }), { code: 'writer_profile_rejected' });
  for (const error of [getter, inherited, { code: 'writer_profile_rejected', sandboxStage: fakePrivate, sandboxReason: fakePrivate, windowsError: fakePrivate },
    { code: 'writer_profile_rejected', sandboxStage: '__proto__', sandboxReason: 'constructor', windowsError: { toString() { touched = true; return '5'; } } }]) {
    const display = writerError(error, 'check');
    assert.match(display, /writer_profile_rejected/);
    assert.ok(!display.includes('检查位置：'));
    assert.ok(!display.includes('Windows 错误：'));
    assert.ok(!display.includes(fakePrivate));
  }
  for (const value of [-1, 0x100000000, 5.5, NaN, Infinity, '5', null]) {
    assert.ok(!writerError({ code: 'writer_profile_rejected', windowsError: value }, 'check').includes('Windows 错误：'));
  }
  const proxy = new Proxy({ code: 'writer_profile_rejected' }, {
    getOwnPropertyDescriptor(target, key) { if (key === 'code') return Object.getOwnPropertyDescriptor(target, key); throw Error(fakePrivate); },
    get() { touched = true; throw Error(fakePrivate); },
  });
  assert.match(writerError(proxy, 'check'), /writer_profile_rejected/);
  assert.equal(touched, false);
});

test('profile diagnostic fields survive export wrapping as immutable approved values', async () => {
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    if (command === 'begin_unicode_export') throw {
      code: 'writer_profile_rejected', sandboxStage: 'marker', sandboxReason: 'marker-identity', windowsError: 0x80070005,
      message: fakePrivate, path: fakePrivate, sid: fakePrivate,
    };
    return undefined as T;
  };
  // A stub writer keeps these bridge-code tests deterministic; the real MuPDF
  // writer is covered by the spike matrix and the Rust writer gates.
  const stubWriter = async (candidate: Uint8Array) => new Uint8Array([...candidate, 10, 37, 37, 69, 79, 70]);
  let caught: unknown;
  try { await desktopSource(descriptor, bridge, stubWriter).exportCopy!(new Uint8Array([37, 80, 68, 70, 45]), new AbortController().signal); }
  catch (error) { caught = error; }
  assert.ok(caught instanceof SourceError);
  const display = writerError(caught, 'finish');
  assert.match(display, /准备另存为未完成/);
  assert.match(display, /检查位置：隔离配置标记/);
  assert.match(display, /原因：隔离配置标记与当前身份不一致/);
  assert.match(display, /2147942405（0x80070005）/);
  assert.ok(!display.includes(fakePrivate));
  for (const key of ['sandboxStage', 'sandboxReason', 'windowsError']) {
    assert.equal(Object.getOwnPropertyDescriptor(caught, key)?.writable, false);
  }
});
