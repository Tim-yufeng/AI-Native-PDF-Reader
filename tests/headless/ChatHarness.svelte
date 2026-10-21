<script lang="ts">
 import AiChat from '/src/components/AiChat.svelte';
 import type {PdfReader} from '/src/lib/pdf-reader';import type {LocalData,DocumentBinding} from '/src/lib/local-data';
 import type {Region} from '/src/lib/ai-context';
 let {reader,data,binding}:{reader:PdfReader;data:LocalData;binding:DocumentBinding}=$props();
 let selected=$state<number[]>([2]),visible=$state(true),region=$state<Region|null>(null);
 export function snapshot(){return {selected:[...selected],visible,region};}
 export function attach(pages:number[],box:Region|null=null){selected=[...pages];region=box;}
 export function show(value:boolean){visible=value;}
</script>
<div class="chat-harness" style="position:relative;display:flex;height:650px;width:420px;--ai-width:350px">
 <AiChat {reader} {data} {binding} {visible} token={1} page={5} total={10} {selected} documentName="Synthetic.pdf" removePage={n=>selected=selected.filter(p=>p!==n)} {region} beginRegion={()=>{}} beginInline={()=>{}} clearRegion={()=>region=null} clearSelected={()=>selected=[]} consumed={()=>{selected=[];region=null;}} close={()=>visible=false} jump={()=>{}} openSource={async()=>{}}/>
</div>
