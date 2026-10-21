<script lang="ts">
  import type { PdfReader } from '../lib/pdf-reader';
  import type { ThumbnailQueue } from '../lib/thumbnail-queue';
  import { AnnotationMode } from 'pdfjs-dist';
  let { reader, page, token, queue,cache }: { reader: PdfReader; page: number; token: number; queue: ThumbnailQueue;cache:Map<string,HTMLCanvasElement> } = $props();
  let canvas: HTMLCanvasElement;
  let error = $state(false);
  $effect(() => {
    const current = reader, number = page;
    void token;
    const controller = new AbortController();
    const density = Math.min(2, window.devicePixelRatio || 1),key=`${token}:${number}:${density}`;
    const display=(image:HTMLCanvasElement)=>{
      canvas.width=image.width;canvas.height=image.height;
      canvas.style.width=`${image.width/density}px`;canvas.style.height=`${image.height/density}px`;
      canvas.getContext('2d')!.drawImage(image,0,0);
    };
    error = false;
    const cached=cache.get(key);
    if(cached){cache.delete(key);cache.set(key,cached);display(cached);return()=>{controller.abort();canvas.width=canvas.height=0;};}
    void queue.schedule(async () => {
      const pdfPage = await current.getPage(number);
      controller.signal.throwIfAborted();
      const original = pdfPage.getViewport({ scale: 1 });
      const cssScale = Math.min(128 / original.width, 140 / original.height);
      const viewport = pdfPage.getViewport({ scale: cssScale * density });
      // Paint into a private buffer. A cancelled/partial render must never
      // replace a complete visible preview with a cleared or black canvas.
      const buffer=document.createElement('canvas');
      buffer.width=Math.ceil(viewport.width);buffer.height=Math.ceil(viewport.height);
      const task = pdfPage.render({ canvas:buffer, viewport, annotationMode: AnnotationMode.ENABLE });
      const cancel = () => task.cancel();
      controller.signal.addEventListener('abort', cancel, { once: true });
      try {
        await task.promise;controller.signal.throwIfAborted();display(buffer);cache.set(key,buffer);
        while(cache.size>24){const oldest=cache.keys().next().value!;const removed=cache.get(oldest)!;cache.delete(oldest);removed.width=removed.height=0;}
      } finally { controller.signal.removeEventListener('abort', cancel); }
    }, controller.signal).catch(() => { if (!controller.signal.aborted) error = true; });
    return () => { controller.abort(); canvas.width = 0; canvas.height = 0; };
  });
</script>
<div class="thumbnail-picture"><canvas bind:this={canvas}></canvas>{#if error}<span>无法预览</span>{/if}</div>
