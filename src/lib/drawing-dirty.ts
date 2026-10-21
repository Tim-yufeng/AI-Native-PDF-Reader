import { AnnotationEditorParamsType } from 'pdfjs-dist';
import type { AnnotationEditorUIManager } from 'pdfjs-dist/types/src/display/editor/tools';

// PDF.js keeps multiple ink strokes in a drawing session before creating its
// serializable editor. Follow its public DRAW_STEP undo commands, not timers or
// pointer distance guesses. All native commands and tool behavior stay intact.
export function trackDrawingDirty(manager: AnnotationEditorUIManager, changed: () => void) {
  const add = manager.addCommands, clean = manager.cleanUndoStack;
  let steps = 0, session = false, attached = true, queued = false;
  const notify = () => {
    if (queued) return; queued = true;
    queueMicrotask(() => { queued = false; if (attached) changed(); });
  };
  const wrappedAdd: typeof add = function (value) {
    const params = value as {type?: number; mustExec?: boolean; cmd: () => void; undo: () => void};
    if (params.type !== AnnotationEditorParamsType.DRAW_STEP) return add.call(manager, params);
    session = true;
    if (!params.mustExec) steps++;
    const result = add.call(manager, { ...params,
      cmd: () => { params.cmd(); steps++; notify(); },
      undo: () => { params.undo(); steps = Math.max(0, steps - 1); notify(); },
    });
    notify(); return result;
  };
  const wrappedClean: typeof clean = function (type) {
    const result = clean.call(manager, type);
    if (type === AnnotationEditorParamsType.DRAW_STEP) {
      steps = 0; session = false;
      // Finalization adds the serializable editor in the same native call.
      notify();
    }
    return result;
  };
  manager.addCommands = wrappedAdd;
  manager.cleanUndoStack = wrappedClean;
  return { hasChanges: () => steps > 0, hasSession: () => session, dispose() {
    attached = false; steps = 0; session = false;
    if (manager.addCommands === wrappedAdd) manager.addCommands = add;
    if (manager.cleanUndoStack === wrappedClean) manager.cleanUndoStack = clean;
  } };
}
