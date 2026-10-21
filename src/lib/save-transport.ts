/** One raw output buffer contains the candidate as an exact prefix. No paths. */
export function saveHeaders(taskId:string,candidateBytes:number,bindingId?:string) {
  return {headers:{'x-ai-save-version':'1','x-ai-save-task':taskId,
    'x-ai-save-candidate-bytes':String(candidateBytes),...(bindingId===undefined?{}:{'x-ai-save-binding':bindingId})}};
}
