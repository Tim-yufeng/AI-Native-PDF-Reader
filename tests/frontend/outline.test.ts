import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bookmarks } from '../../src/lib/outline.ts';
test('PDF outline retains internal targets and text; external actions are inert', () => {
  const outline = bookmarks([{title:'Chapter <script>',dest:'chapter',items:[{title:'Page 2',dest:[1,{name:'Fit'}]}]}, {title:'External',url:'file:///private'}]);
  assert.equal(outline[0].title,'Chapter <script>'); assert.equal(outline[1].depth,1); assert.equal(outline[2].destination,null);
});
test('untrusted cyclic or huge outlines are bounded', () => {
  const item: {title:string;items?:unknown[]} = {title:'cycle'}; item.items=[item];
  assert.equal(bookmarks([item]).length,21); assert.equal(bookmarks(Array(2500).fill(item)).length,2000);
  assert.deepEqual(bookmarks(null),[]);
});
