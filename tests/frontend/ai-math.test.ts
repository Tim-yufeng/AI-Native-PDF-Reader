import test from 'node:test';
import assert from 'node:assert/strict';
import { splitMath } from '../../src/lib/ai-math.ts';
test('AI math accepts common inline and display delimiters without changing code samples', () => {
  const sample = String.raw`$a$ $$b$$ \(c\) \[d\]`;
  assert.deepEqual(splitMath(sample).formulas, [{ value: 'a', display: false }, { value: 'b', display: true }, { value: 'c', display: false }, { value: 'd', display: true }]);
  const code = '`$x$`\n```tex\n$$y$$\n```'; assert.equal(splitMath(code).source, code); assert.deepEqual(splitMath(code).formulas, []);
});
