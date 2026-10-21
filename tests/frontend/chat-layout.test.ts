import test from 'node:test';import assert from 'node:assert/strict';
import {composerHeight,sessionRoot} from '../../src/lib/chat-layout.ts';
import {contextError,contextPages,defaultContext} from '../../src/lib/preferences.ts';
import {sourceReason} from '../../src/lib/chat-input.ts';
test('composer sizing reserves messages, clamps narrow windows and handles invalid stored layouts',()=>{
 assert.equal(composerHeight(NaN,700),220);assert.equal(composerHeight(9999,700),420);assert.equal(composerHeight(-100,700),140);assert.equal(composerHeight(220,250),100);
});
test('independent roots differ from editing branches, including edits of the root prompt',()=>{
 const records=[{id:'A'},{id:'answer',parent:'A'},{id:'edited',editedFrom:'A'},{id:'newAnswer',parent:'edited'},{id:'B'},{id:'orphan',parent:'missing'},{id:'cycle1',parent:'cycle2'},{id:'cycle2',parent:'cycle1'}];
 assert.equal(sessionRoot(records,'answer'),'A');assert.equal(sessionRoot(records,'newAnswer'),'A');assert.equal(sessionRoot(records,'B'),'B');assert.equal(sessionRoot(records,'orphan'),null);assert.equal(sessionRoot(records,'cycle1'),null);
});
test('optional neighbors default to zero, stay bounded and do not expand pinned or retrieved pages',()=>{
 assert.deepEqual(defaultContext(),{schemaVersion:1,beforePages:0,afterPages:0,automaticCurrentPage:true,inlineFullDocument:true,inlineAnswerLimit:20});assert.deepEqual(contextPages(18,[2,8],undefined,30,defaultContext()),[2,8]);
 assert.deepEqual(contextPages(18,[2,8],undefined,30,{schemaVersion:1,beforePages:1,afterPages:1,inlineAnswerLimit:20}),[2,8,1,3,7,9]);
 assert.deepEqual(contextPages(1,[],30,30,{schemaVersion:1,beforePages:1,afterPages:5,inlineAnswerLimit:20}),[30,29]);
 for(const n of [-1,6,1.5,NaN])assert.ok(contextError({schemaVersion:1,beforePages:n,afterPages:0,inlineAnswerLimit:20}));
 assert.equal(sourceReason(['manual','neighbor','retrieved']),'本轮选页 · 邻近页 · 全文检索');
});
