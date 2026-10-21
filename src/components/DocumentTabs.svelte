<script lang="ts">
  import type { DocumentTab } from '../lib/document-tabs';
  let { tabs, activeId, activate, close }: { tabs: DocumentTab[]; activeId: number | null; activate: (id: number) => void; close: (id: number) => void } = $props();
  let container: HTMLDivElement;
  $effect(() => {
    // Keep the active label visible without scrolling the surrounding reader.
    const current = activeId;
    void tabs.length;
    if (!container || current === null) return;
    const active = container.querySelector<HTMLElement>('.document-tab.active');
    if (!active) return;
    const left = active.offsetLeft;
    const right = left + active.offsetWidth;
    if (left < container.scrollLeft) container.scrollLeft = Math.max(0, left - 4);
    else if (right > container.scrollLeft + container.clientWidth) {
      container.scrollLeft = Math.max(0, right - container.clientWidth + 4);
    }
  });
</script>
<div class="document-tabs" aria-label="已打开文档" bind:this={container}>
  {#each tabs as tab (tab.id)}
    <div class:active={tab.id === activeId} class="document-tab">
      <button class="tab-name" aria-pressed={tab.id === activeId} aria-label={`切换文档 ${tab.id}：${tab.source.name}`} title={tab.source.name} onclick={() => activate(tab.id)}>
        <svg class="file-icon" width="15" height="17" viewBox="0 0 18 20" fill="none" stroke="currentColor" stroke-width="1.35" aria-hidden="true">
          <path d="M4 1.75h6.5L15 6.25v11.5a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5V2.25a.5.5 0 0 1 .5-.5Z" />
          <path d="M10.5 1.75v4.5H15M6 10h6M6 13h5" />
        </svg>
        <span>{tab.dirty ? '● ' : ''}{tab.source.name}</span>
      </button>
      <button class="tab-close" aria-label={`关闭文档 ${tab.id}：${tab.source.name}`} title="关闭文档" onclick={() => close(tab.id)}>
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="m4 4 8 8M12 4l-8 8" /></svg>
      </button>
    </div>
  {/each}
</div>

<style>
  .document-tabs {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 38px;
    align-items: stretch;
    gap: 3px;
    overflow-x: auto;
    overflow-y: hidden;
    padding: 4px 4px 0;
    background: transparent;
    scrollbar-width: thin;
  }
  .document-tab {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    min-width: 112px;
    max-width: 215px;
    border: 1px solid transparent;
    border-bottom: 0;
    border-radius: 6px 6px 0 0;
    background: transparent;
    color: #68717c;
  }
  .document-tab:hover { background: #e9edf1; }
  .document-tab.active {
    background: #fff;
    border-color: #dce1e7;
    color: #263342;
    box-shadow: inset 0 2px 0 #527493;
  }
  .document-tab button { border: 0; border-radius: 4px; background: transparent; color: inherit; }
  .tab-name {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 7px;
    min-width: 0;
    padding: 7px 6px 7px 10px;
    text-align: left;
    font-size: 12px;
  }
  .tab-name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-icon { flex-shrink: 0; color: #6b8093; }
  .tab-close { display: grid; place-items: center; width: 26px; height: 26px; padding: 0; margin-right: 4px; flex-shrink: 0; }
  .tab-close:hover { background: #e8edf2; color: #263342; }
  button:focus-visible { outline: 2px solid #527493; outline-offset: -2px; }
</style>
