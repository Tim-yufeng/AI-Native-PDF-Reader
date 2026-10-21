<script lang="ts">
  import type { Bookmark } from '../lib/outline';
  let { items, jump, close }: { items: Bookmark[]; jump: (destination: string | unknown[]) => void; close: () => void } = $props();
</script>
<aside class="bookmark-sidebar thumbnail-sidebar" aria-label="文档书签">
  <div class="sidebar-title"><h2>书签</h2><button class="icon-button" aria-label="收起书签" onclick={close}>‹</button></div>
  <div class="bookmark-scroll">
    {#each items as item}
      <button class="bookmark-item" style:padding-left={`${12 + Math.min(item.depth, 6) * 12}px`} disabled={!item.destination} onclick={() => item.destination && jump(item.destination)}>{item.title}</button>
    {:else}<p>这份 PDF 没有内置书签。</p>{/each}
  </div>
</aside>
<style>
  aside { width: 220px; flex-shrink: 0; display: flex; flex-direction: column; background: #f6f7f9; border-right: 1px solid #dce1e7; }
  .sidebar-title { display:flex; align-items:center; justify-content:space-between; padding:5px 9px 5px 13px; min-height:40px; }
  h2 { margin:0; font-size:12px; font-weight:500; }
  .bookmark-scroll { flex:1; min-height:0; overflow:auto; }
  .bookmark-item { width:100%; border:0; background:transparent; padding:10px 12px; text-align:left; font-size:12px; overflow-wrap:anywhere; }
  .bookmark-item:hover:not(:disabled) { background:#e8eef5; }
  p { padding:12px; color:#738292; font-size:12px; }
</style>
