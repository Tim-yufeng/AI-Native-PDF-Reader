// Chromium precision-touchpad pinch is a Ctrl+wheel event. The PDFViewer
// component exposes scaling, but Mozilla's application-level wheel handler is
// not part of that component. Keep browser/UI zoom separate from PDF scaling.
interface WheelZoomOptions {
  ready: () => boolean;
  scale: () => number;
  apply: (factor: number, origin: [number, number]) => void;
  signal: AbortSignal;
}

export function attachPdfWheelZoom(container: HTMLElement, options: WheelZoomOptions): { reset(): void } {
  const host = container.ownerDocument.defaultView!;
  // With native pinch delivery enabled, suppress browser page scaling outside
  // the PDF too. The reading handler runs first; this does not apply PDF zoom.
  host.addEventListener('wheel', event => { if (event.ctrlKey && event.cancelable) event.preventDefault(); }, { passive: false, signal: options.signal });
  let controlDown = false, frame = 0, desired: number | null = null;
  let applied = 0, lastInput = 0, origin: [number, number] = [0, 0];
  const reset = () => {
    if (frame) host.cancelAnimationFrame(frame);
    frame = 0; desired = null; applied = 0; lastInput = 0;
  };
  host.addEventListener('keydown', event => { controlDown = event.ctrlKey; }, { signal: options.signal });
  host.addEventListener('keyup', event => { controlDown = event.ctrlKey; }, { signal: options.signal });
  host.addEventListener('blur', () => { controlDown = false; reset(); }, { signal: options.signal });
  container.addEventListener('pointerdown', reset, { passive: true, signal: options.signal });
  container.addEventListener('wheel', event => {
    // Leave ordinary two-finger/horizontal scrolling and modified scrolling
    // alone. A non-cancelable event cannot safely replace native zoom.
    if (!event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) { reset(); return; }
    if (event.defaultPrevented || !event.cancelable) return;
    event.preventDefault();
    const mode = event.deltaMode;
    if (!options.ready() || !Number.isFinite(event.deltaY) || !Number.isFinite(event.clientX) || !Number.isFinite(event.clientY)) { reset(); return; }
    // Read mode before delta, also matching Firefox's unit-conversion contract.
    const delta = event.deltaY;
    if (!delta) return;
    const pinch = mode === 0 && !controlDown && Math.abs(delta) < 50;
    const ticks = mode === 0 ? delta / 100 : Math.sign(delta);
    const exponent = Math.max(-0.5, Math.min(0.5, pinch ? -delta / 100 : -ticks * Math.log(1.1)));
    const current = options.scale();
    if (!Number.isFinite(current) || current <= 0) { reset(); return; }
    if (desired === null || Math.abs(current - applied) > 0.00001 || event.timeStamp - lastInput > 250) desired = current;
    // Retain sub-percent pinch deltas instead of losing them to PDF.js rounding.
    desired = Math.min(4, Math.max(0.25, desired * Math.exp(exponent)));
    applied = current; lastInput = event.timeStamp; origin = [event.clientX, event.clientY];
    if (frame) return;
    frame = host.requestAnimationFrame(() => {
      frame = 0;
      if (!options.ready() || desired === null) { reset(); return; }
      const before = options.scale();
      if (Math.abs(before - applied) > 0.00001) { reset(); return; }
      const next = Math.min(4, Math.max(0.25, Math.round(desired * 100) / 100));
      if (next !== before) options.apply(next / before, origin);
      applied = options.scale();
    });
  }, { capture: true, passive: false, signal: options.signal });
  options.signal.addEventListener('abort', reset, { once: true });
  return { reset };
}
