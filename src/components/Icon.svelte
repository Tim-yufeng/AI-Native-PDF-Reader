<script lang="ts">
  // Small, local line icons. No remote assets or UI framework.
  type Name = 'file' | 'folder' | 'close' | 'thumbs' | 'search' | 'previous' | 'next' | 'down' | 'minus' | 'plus' | 'more' | 'width' | 'fit' | 'help' | 'pen' | 'eraser' | 'save' | 'undo' | 'redo' | 'settings' | 'send' | 'stop' | 'refresh'|'select'|'region'|'text'|'highlight'|'image';
  let { name }: { name: Name } = $props();
  const paths: Record<Name, string> = {
    file: 'M6 3h8l4 4v14H6z M14 3v5h4 M9 12h6 M9 16h6',
    folder: 'M3 7h7l2 2h9v11H3z M3 7V4h6l2 3',
    close: 'm6 6 12 12 M18 6 6 18',
    thumbs: 'M5 3h14v18H5z M10 3v18 M5 9h5 M5 15h5',
    search: 'M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0 M16 16l4.5 4.5',
    previous: 'm14 6-6 6 6 6', next: 'm10 6 6 6-6 6', down: 'm7 10 5 5 5-5',
    minus: 'M5 12h14', plus: 'M12 5v14 M5 12h14',
    more: 'M6 12a1 1 0 1 1-2 0 1 1 0 0 1 2 0 M13 12a1 1 0 1 1-2 0 1 1 0 0 1 2 0 M20 12a1 1 0 1 1-2 0 1 1 0 0 1 2 0',
    width: 'M4 5v14 M20 5v14 M4 12h16 m-12-4-4 4 4 4 m8-8 4 4-4 4',
    fit: 'M6 3h12v18H6z M9 8h6 M9 12h6 M9 16h4',
    help: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0 M9.5 8.5a2.5 2.5 0 1 1 3.3 2.4c-.8.3-.8 1.1-.8 2.1 M12 16v.2',
    pen: 'm4 17 12-12 3 3-12 12-4 1z M14 7l3 3',
    eraser: 'm3 14 10-10 8 8-8 8H9z M8 9l8 8 M13 20h8',
    save: 'M4 3h13l3 3v15H4z M8 3v6h8V3 M8 21v-8h8v8',
    undo: 'm8 5-5 5 5 5 M3 10h10a6 6 0 0 1 0 12',
    redo: 'm16 5 5 5-5 5 M21 10H11a6 6 0 0 0 0 12',
    settings: 'M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
    send: 'M12 20V4 m-7 7 7-7 7 7',
    stop: 'M6 6h12v12H6z',
    refresh: 'M20 7v5h-5 M20 12a8 8 0 1 0-2 5',
    select: 'M5 3v16l4-4 3 6 3-2-3-6h6z',
    region: 'M3 8V3h5 M16 3h5v5 M21 16v5h-5 M8 21H3v-5 M8 8h8v8H8z',
    text: 'M5 5h14 M12 5v14 M8 19h8',
    highlight: 'm6 14 8-10 6 5-8 10-6-5z M5 16l3 3-4 2H2z M3 22h18',
    image: 'M3 4h18v16H3z M6 16l4-5 3 3 3-4 4 6 M9 8h.1',
  };
</script>

<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>

<style>
  svg { width: 18px; height: 18px; flex: 0 0 auto; }
</style>
