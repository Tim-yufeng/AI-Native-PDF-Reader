// Observation-only transforms for the existing isolated Release App lab.
// No production source is edited; each original operation still runs once.
export function saveLabInstrumentation(){return {
  name:'isolated-save-evidence',enforce:'pre',
  transform(code,id){
    if(id.includes('?'))return; // Svelte virtual CSS is not component source.
    const file=id.replaceAll('\\','/').split('?')[0];
    const replace=(before,after)=>{if(code.split(before).length!==2&&code.split(before.replaceAll('\n','\r\n')).length===2)before=before.replaceAll('\n','\r\n');if(code.split(before).length!==2)throw Error(`Save evidence target changed: ${file}: ${before.slice(0,60)}`);code=code.replace(before,after);};
    if(file.endsWith('/src/App.svelte')){
      replace('await checkpointDraft();remember();',"await globalThis.__SAVE_PROBE__.measure('app.checkpoint',()=>checkpointDraft());remember();");
      replace('await Promise.all([draftWrites.get(tab.id)?.catch(()=>{}),positionWrites.get(tab.id)?.catch(()=>{}),bindingTasks.get(tab.id)?.catch(()=>{})]);',"await globalThis.__SAVE_PROBE__.measure('app.awaitWrites',()=>Promise.all([draftWrites.get(tab.id)?.catch(()=>{}),positionWrites.get(tab.id)?.catch(()=>{}),bindingTasks.get(tab.id)?.catch(()=>{})]));");
      replace("await invoke('check_unicode_writer');\n      const candidate=await current.testWriterBytes();", "await globalThis.__SAVE_PROBE__.measure('app.writerCheck',()=>invoke('check_unicode_writer'));\n      const candidate=await globalThis.__SAVE_PROBE__.measure('app.serialize',()=>current.testWriterBytes());");
      replace('await oldSource.release?.();await loadActive();',"await globalThis.__SAVE_PROBE__.measure('app.reload',async()=>{await oldSource.release?.();await loadActive();});");
    }else if(file.endsWith('/src/lib/desktop-source.ts')){
      replace('saveHeaders(task,bytes.byteLength,bindingId)',"globalThis.__SAVE_PROBE__.headers(()=>saveHeaders(task,bytes.byteLength,bindingId),bytes.byteLength,output.byteLength)");
      replace('return result.bytes;',"globalThis.__SAVE_PROBE__.writer({targeted:result.targeted,asserted:result.asserted,embeddedFont:result.embeddedFont,fontBytes:result.fontBytes,candidateBytes:candidate.length,outputBytes:result.bytes.length});return result.bytes;");
    }else if(file.endsWith('/src/lib/original-output.ts')){
      replace('const before=getDocument({...options,data:candidate.slice()}),after=getDocument({...options,data:output.slice()});',"globalThis.__SAVE_PROBE__.count('validation.documents',2);const before=getDocument({...options,data:candidate.slice()}),after=getDocument({...options,data:output.slice()});");
      replace('signal.throwIfAborted();const [x,y]=await Promise.all([a.getPage(page),b.getPage(page)]);',"globalThis.__SAVE_PROBE__.count('validation.pages',1);globalThis.__SAVE_PROBE__.count('validation.annotations',2);globalThis.__SAVE_PROBE__.count('validation.text',2);signal.throwIfAborted();const [x,y]=await Promise.all([a.getPage(page),b.getPage(page)]);");
      replace('await y.getOperatorList();x.cleanup();y.cleanup();',"globalThis.__SAVE_PROBE__.count('validation.operatorLists',1);await y.getOperatorList();x.cleanup();y.cleanup();");
    }else return;
    return code;
  }
};}
