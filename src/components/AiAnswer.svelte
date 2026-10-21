<script lang="ts">
  import { renderAiAnswer } from '../lib/ai-answer';
  import 'katex/dist/katex.min.css';
  let { text }: { text: string } = $props();
  // Rendering and sanitization live in one testable module; this component only
  // mounts the reviewed result. See src/lib/ai-answer.ts and its unit test.
  const html = $derived(renderAiAnswer(text));
</script>
<div class="ai-answer">{@html html}</div>
<style>
  .ai-answer { line-height: 1.65; overflow-wrap: anywhere; font-size: 13px; }
  .ai-answer :global(p) { margin: .5em 0; }
  .ai-answer :global(pre) { background: #f0f3f6; overflow: auto; padding: 9px; border-radius: 4px; }
  .ai-answer :global(code) { font-size: 12px; }
  .ai-answer :global(.katex-display) { overflow: auto; padding: 3px; }
  .ai-answer :global(table) { border-collapse: collapse; font-size: 12px; }
  .ai-answer :global(td),.ai-answer :global(th) { border: 1px solid #ddd; padding: 4px; }
</style>
