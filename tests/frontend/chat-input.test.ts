import test from 'node:test';
import assert from 'node:assert/strict';
import { enterSends, sourceReason } from '../../src/lib/chat-input.ts';
test('Enter sends only an intentional non-composing unmodified key; Shift+Enter remains a newline',()=>{
 const key={key:'Enter',shiftKey:false,ctrlKey:false,altKey:false,metaKey:false,isComposing:false};
 assert.equal(enterSends(key),true);
 for(const field of ['shiftKey','ctrlKey','altKey','metaKey','isComposing','repeat'])assert.equal(enterSends({...key,[field]:true}),false);
 assert.equal(enterSends({...key,keyCode:229}),false);assert.equal(enterSends(key,true),false);assert.equal(enterSends(key,false,true),false);
 assert.equal(sourceReason(['manual','pinned']),'本轮选页 · 固定页面');
});
