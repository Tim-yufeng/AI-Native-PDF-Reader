import test from 'node:test';
import assert from 'node:assert/strict';
import { attachPdfWheelZoom } from '../../src/lib/wheel-zoom.ts';

class Host extends EventTarget {
  frames = new Map<number, FrameRequestCallback>(); serial = 0;
  requestAnimationFrame(callback: FrameRequestCallback) { const id = ++this.serial; this.frames.set(id, callback); return id; }
  cancelAnimationFrame(id: number) { this.frames.delete(id); }
  flush() { const frames = [...this.frames.values()]; this.frames.clear(); for (const run of frames) run(0); }
}
function fixture() {
  const host = new Host(), container = new EventTarget(), abort = new AbortController();
  Object.assign(container, { ownerDocument: { defaultView: host } });
  let scale = 1, ready = true;
  const calls: { factor: number; origin: [number, number] }[] = [];
  const controller = attachPdfWheelZoom(container as HTMLElement, {
    signal: abort.signal, ready: () => ready, scale: () => scale,
    apply: (factor, origin) => { calls.push({ factor, origin }); scale = Math.round(scale * factor * 100) / 100; },
  });
  const wheel = (values: object = {}, cancelable = true) => {
    const event = Object.assign(new Event('wheel', { cancelable }), {
      ctrlKey: true, altKey: false, metaKey: false, shiftKey: false,
      deltaY: -1, deltaMode: 0, clientX: 380, clientY: 260, ...values,
    });
    container.dispatchEvent(event); return event;
  };
  return { host, container, abort, calls, controller, wheel, scale: () => scale, setScale: (value: number) => scale = value, ready: (value: boolean) => ready = value };
}

test('ordinary scrolling is untouched; fractional pinch coalesces around the pointer and physical Ctrl+wheel uses a bounded step', () => {
  const f = fixture();
  for (const values of [{ ctrlKey: false }, { shiftKey: true }, { altKey: true }, { metaKey: true }]) assert.equal(f.wheel(values).defaultPrevented, false);
  assert.equal(f.wheel({}, false).defaultPrevented, false); f.host.flush(); assert.equal(f.calls.length, 0);
  for (let i = 0; i < 20; i++) assert.equal(f.wheel({ deltaY: -0.05 }).defaultPrevented, true);
  assert.equal(f.host.frames.size, 1); f.host.flush(); assert.equal(f.calls.length, 1); assert.equal(f.scale(), 1.01);
  assert.deepEqual(f.calls[0].origin, [380, 260]);
  f.host.dispatchEvent(Object.assign(new Event('keydown'), { ctrlKey: true }));
  f.wheel({ deltaY: -100 }); f.host.flush(); assert.equal(f.scale(), 1.11);
  f.wheel({ deltaY: 3, deltaMode: 1 }); f.host.flush(); assert.equal(f.scale(), 1.01);
  f.abort.abort();
});

test('PDF bounds, unloaded documents, document reset and disposal cannot apply stale zoom to a replacement', () => {
  const f = fixture();
  f.setScale(3.99); f.wheel({ deltaY: -10000 }); f.host.flush(); assert.equal(f.scale(), 4);
  f.setScale(0.25); f.wheel({ deltaY: 10000 }); f.host.flush(); assert.equal(f.scale(), 0.25);
  f.ready(false); assert.equal(f.wheel().defaultPrevented, true); f.host.flush(); assert.equal(f.calls.length, 1);
  f.ready(true); f.setScale(1); f.wheel(); f.controller.reset(); f.setScale(2); f.host.flush(); assert.equal(f.scale(), 2);
  f.wheel(); f.setScale(3); f.host.flush(); assert.equal(f.scale(), 3); // A newer toolbar zoom wins.
  f.wheel(); f.abort.abort(); f.host.flush(); assert.equal(f.scale(), 3); assert.equal(f.wheel().defaultPrevented, false);
});
