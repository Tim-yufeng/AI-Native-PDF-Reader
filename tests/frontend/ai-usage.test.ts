import test from 'node:test';
import assert from 'node:assert/strict';
import { tokenUsage } from '../../src/lib/ai-usage.ts';

test('reported token counts survive missing, partial, zero and Mock reports without inventing usage', () => {
  assert.equal(tokenUsage({promptTokens:1200,completionTokens:34,totalTokens:1234}), 'Token：输入 1,200 · 输出 34 · 合计 1,234');
  assert.equal(tokenUsage({promptTokens:10,completionTokens:5}), 'Token：输入 10 · 输出 5 · 合计 15');
  assert.equal(tokenUsage({promptTokens:0,completionTokens:0}), 'Token：输入 0 · 输出 0 · 合计 0');
  assert.equal(tokenUsage({totalTokens:15}), 'Token：输入 — · 输出 — · 合计 15');
  for (const value of [undefined,null,{}, {promptTokens:-1,completionTokens:'12'}, {promptTokens:Infinity}, {totalTokens:Number.MAX_SAFE_INTEGER+1}]) assert.equal(tokenUsage(value), 'Token：未返回用量');
  assert.equal(tokenUsage({mock:true}), '本地 Mock · 无 API 消耗');
});
