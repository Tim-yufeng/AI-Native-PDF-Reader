import { extractPages, type PageReader } from './ai-context.ts';
import { contextPages, type ContextSettings } from './preferences.ts';

/** Called only for an intentional request. Full indexing is all-or-error;
 * partial batches are never advertised as a completed document. */
export async function indexInlineContext(reader:PageReader,total:number,page:number,sourcePage:number|undefined,
  settings:ContextSettings,signal:AbortSignal,put:(pages:{page:number;text:string}[])=>Promise<void>,progress:(page:number)=>void) {
  if(settings.inlineFullDocument!==false){await extractPages(reader,total,signal,put,progress);return;}
  const pages=contextPages(page,[],sourcePage,total,{...settings,automaticCurrentPage:true});
  if(!pages.includes(page))pages.push(page);
  for(const n of pages){signal.throwIfAborted();const content=await(await reader.getPage(n)).getTextContent();signal.throwIfAborted();
    const text=content.items.map(item=>'str'in item?item.str+(item.hasEOL?'\n':' '):'').join('');
    if(new TextEncoder().encode(text).length>32768)throw Error('本页文字超过索引容量，未发送不完整来源。');
    await put([{page:n,text}]);progress(n);
  }
}
