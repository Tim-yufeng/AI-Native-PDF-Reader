<script lang="ts">
  import { onMount } from 'svelte';
  import type { PdfReader } from '../lib/pdf-reader';
  import { THUMBNAIL_ROW, thumbnailWindow } from '../lib/navigation';
  import { ThumbnailQueue } from '../lib/thumbnail-queue';
  import Thumbnail from './Thumbnail.svelte';
  let { reader, token, total, page, jump, close, selected = [], toggleSelected }: { reader: PdfReader; token: number; total: number; page: number; jump: (page: number) => void; close?: () => void; selected?: number[]; toggleSelected?: (page: number) => void } = $props();
  let container: HTMLDivElement;
  let top = $state(0), height = $state(600);
  const queue = new ThumbnailQueue();
  // Session-only LRU, at most about 7 MiB at DPR 2. Never persisted or shared
  // with document/history data; keys include the reader's document token.
  const cache = new Map<string,HTMLCanvasElement>();
  const visible = $derived(thumbnailWindow(top, height, total));
  onMount(() => {
    const resize = new ResizeObserver(() => { height = container.clientHeight; });
    resize.observe(container); height = container.clientHeight;
    return () => resize.disconnect();
  });
  $effect(() => {
    const target = (page - 1) * THUMBNAIL_ROW;
    if (!container || page < 1) return;
    // Read scroll offsets directly so manual sidebar scrolling stays independent.
    if (target < container.scrollTop || target + THUMBNAIL_ROW > container.scrollTop + container.clientHeight) {
      container.scrollTop = target;
    }
  });
</script>
<aside id="thumbnails" class="thumbnail-sidebar" aria-label="页面缩略图">
  <div class="sidebar-title">
    <h2>缩略图</h2>
    {#if close}
      <button class="sidebar-close" aria-label="收起缩略图" title="收起缩略图" onclick={close}>
        <svg width="15" height="15" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m10.5 5-4 4 4 4" /></svg>
      </button>
    {/if}
  </div>
  <div class="thumbnail-scroll" bind:this={container} onscroll={() => { top = container.scrollTop; }}>
    <div class="thumbnail-space" style:height={`${total * THUMBNAIL_ROW}px`}>
      {#each visible as number (`${token}:${number}`)}
        <div class="thumbnail-row" style:top={`${(number - 1) * THUMBNAIL_ROW}px`}>
        {#if toggleSelected}<label class="context-check" title="选择为 AI 页面上下文"><input type="checkbox" aria-label={`选择第 ${number} 页作为 AI 上下文`} checked={selected.includes(number)} onchange={() => toggleSelected?.(number)} /></label>{/if}
        <button class="thumbnail-item" class:current={number === page} aria-label={`跳转第 ${number} 页`}
          aria-current={number === page ? 'page' : undefined} onclick={() => jump(number)}>
          <Thumbnail {reader} page={number} {token} {queue} {cache} />
          <span>{number}</span>
        </button></div>
      {/each}
    </div>
  </div>
</aside>

<style>
  .thumbnail-sidebar {
    width: var(--thumbnail-width,164px);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: #f6f7f9;
    border-right: 1px solid #dce1e7;
  }
  .sidebar-title { display: flex; align-items: center; justify-content: space-between; min-height: 40px; padding: 5px 9px 5px 13px; color: #68717c; }
  h2 { margin: 0; font-size: 12px; font-weight: 500; }
  .sidebar-close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 0; border-radius: 4px; color: #68717c; background: transparent; }
  .sidebar-close:hover { color: #263342; background: #e8edf2; }
  .thumbnail-scroll { overflow: auto; overflow-x: hidden; flex: 1; min-height: 0; scrollbar-width: thin; }
  .thumbnail-space { position: relative; }
  .thumbnail-row { position:absolute; left:0; width:100%; height:170px; }
  .context-check { position:absolute; left:13px; top:8px; z-index:1; padding:2px; background:#ffffffdd; border-radius:3px; }
  .thumbnail-item {
    position: absolute;
    left: 8px;
    width: 142px;
    height: 170px;
    padding: 5px 3px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    border: 1px solid transparent;
    border-radius: 5px;
    color: #68717c;
    background: transparent;
  }
  .thumbnail-item:hover { background: #edf0f4; }
  .thumbnail-item.current { border-color: #7290ab; background: #e8eef5; color: #2f5577; }
  .thumbnail-item > span { min-width: 24px; font-size: 11px; line-height: 16px; text-align: center; }
  .thumbnail-item :global(canvas) { box-shadow: 0 1px 3px #172b4526; }
  button:focus-visible { outline: 2px solid #527493; outline-offset: -2px; }
</style>
