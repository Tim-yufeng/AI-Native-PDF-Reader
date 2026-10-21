import test from 'node:test';
import assert from 'node:assert/strict';
import {sameSavedAnnotations} from '../../src/lib/original-output.ts';
const before=[{id:'8R',type:'Highlight',text:'',rect:[159.6707943678,689.2775989473,225.215995878,707.5727988482]},
  {id:'9R',type:'Ink',text:'',rect:[252.6212242842,493.2812182009,348.0587636232,507.8387839794]}];
const after=[{id:'8R',type:'Highlight',text:'',rect:[159.67079,689.2776,225.21599,707.5728]},
  {id:'9R',type:'Ink',text:'',rect:[252.62123,493.28123,348.05879,507.83879]}];
test('actual Release editor coordinates retain geometry within a single float32 ULP and a hard cap',()=>{
 assert.notDeepEqual(before,after);assert.ok(sameSavedAnnotations(before,after));assert.ok(sameSavedAnnotations(before,before));
});
test('position changes, text, type, identity, order and invalid geometry remain refused',()=>{
 for(const mutate of [
  (v:typeof after)=>{v[0].rect[0]+=.001;},(v:typeof after)=>{v[0].rect[0]+=1;},
  (v:typeof after)=>{v[0].text='changed';},(v:typeof after)=>{v[0].type='Ink';},
  (v:typeof after)=>{v[0].id='99R';},(v:typeof after)=>{v.reverse();},
  (v:typeof after)=>{v[0].rect[0]=NaN;},(v:typeof after)=>{v[0].rect[0]=Infinity;},
  (v:typeof after)=>{v[0].rect.pop();},(v:typeof after)=>{v.pop();},
 ]){const changed=structuredClone(after);mutate(changed);assert.equal(sameSavedAnnotations(before,changed),false);}
 // Identical float32 buckets alone cannot accept a visible shift at huge coordinates.
 const a=[{id:'1R',type:'Ink',text:'',rect:[10000000,0,10000100,100]}],b=structuredClone(a);b[0].rect[0]+=.25;
 assert.equal(Math.fround(a[0].rect[0]),Math.fround(b[0].rect[0]));assert.equal(sameSavedAnnotations(a,b),false);
});
