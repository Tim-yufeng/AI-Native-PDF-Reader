<script lang="ts">
  import InlineExplanation from '../../src/components/InlineExplanation.svelte';
  import type { PdfReader } from '../../src/lib/pdf-reader';
  import type { DocumentBinding } from '../../src/lib/local-data';
  import type { Region } from '../../src/lib/ai-context';
  let {reader,container,token,source,initialBox,closed}:{
    reader:PdfReader;container:HTMLElement;token:number;source:Region;initialBox:Region;
    closed:(reason:'cancelled'|'confirmed')=>void;
  }=$props();
  const binding:DocumentBinding={
    bindingId:'00000000-0000-4000-8000-000000000001',
    documentId:'00000000-0000-4000-8000-000000000002',
    revisionId:'00000000-0000-4000-8000-000000000003',
  };
  let box=$state<Region>({page:initialBox.page,rect:[...initialBox.rect]});
  let active=$state(true),confirmed=false;
  export function snapshot():Region{return {page:box.page,rect:[...box.rect]};}
  export function resetBox(next:Region){box={page:next.page,rect:[...next.rect]};}
  function finish(){active=false;closed(confirmed?'confirmed':'cancelled');}
</script>

{#if active}
  <InlineExplanation {reader} {binding} {container} {token} {source} {box}
    changed={next=>box={page:next.page,rect:[...next.rect]}}
    place={()=>{}} close={finish} busyChanged={busy=>{if(busy)confirmed=true;}}/>
{/if}
