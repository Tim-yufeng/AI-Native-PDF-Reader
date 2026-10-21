import test from 'node:test';import assert from 'node:assert/strict';
import { defaultLayout, validLayout, resizedLayout } from '../../src/lib/reader-layout.ts';
test('split widths retain a reading center, clamp extremes and safely restore preferences',()=>{
 const l=defaultLayout();assert.deepEqual(validLayout(null),l);assert.deepEqual(validLayout({left:NaN,right:400,aiOpen:true}),l);
 const wide=resizedLayout(l,'right',9999,1000,true);assert.equal(wide.right,466);assert.equal(1000-wide.left-wide.right,370);
 assert.equal(resizedLayout(l,'left',-500,1200,true).left,154);
 assert.equal(validLayout({left:700,right:10,aiOpen:true}).right,280);
 assert.deepEqual(validLayout(JSON.parse(JSON.stringify(wide))),wide);
});
