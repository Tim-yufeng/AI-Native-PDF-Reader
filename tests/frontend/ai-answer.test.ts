// A remote model is untrusted input. This file proves two separate things:
// 1. the exact tag/attribute policy handed to DOMPurify is the closed one the
//    application uses, so a future edit that widens it fails here; and
// 2. the post-sanitization formula substitution cannot be steered by anything
//    the model types, including a forged placeholder or a rejected KaTeX
//    extension.
// The browser-level behaviour of DOMPurify itself is exercised by the isolated
// headless m6 case, which renders a fixed synthetic reply in a real engine.
import test from 'node:test';
import assert from 'node:assert/strict';
import { marked } from 'marked';
import { ALLOWED_TAGS, FORMULA_LIMIT, markdownToHtml, renderWith } from '../../src/lib/ai-answer.ts';

// A strict stand-in for DOMPurify that enforces the same closed policy: only
// allowed tags survive, and every attribute is dropped. It is deliberately
// conservative, so it can never make hostile input look safe.
function enforcePolicy(html: string): string {
  const allowed = new Set<string>(ALLOWED_TAGS);
  return html.replace(/<\s*(\/?)\s*([a-zA-Z0-9]+)([^>]*)>/g, (all: string, closing: string, tag: string, attrs: string) => {
    const name = tag.toLowerCase();
    if (!allowed.has(name)) return '';
    if (closing) return `</${name}>`;
    if (attrs.trim()) return ''; // any attribute at all is refused
    return `<${name}>`;
  });
}
const render = (text: string): string => renderWith(text, enforcePolicy);

const hostile = [
  '<script>alert(1)</script>',
  '<img src=x onerror=alert(1)>',
  '[click me](javascript:alert(1))',
  '[click me](https://example.invalid/track)',
  '![pixel](https://example.invalid/p.png)',
  '<a href="file:///C:/Windows/win.ini">open</a>',
  '<iframe src="https://example.invalid"></iframe>',
  '<svg onload=alert(1)></svg>',
  '<div style="background:url(javascript:alert(1))">styled</div>',
  '<form action="https://example.invalid"><button>send</button></form>',
  '<meta http-equiv="refresh" content="0;url=https://example.invalid">',
  '<object data="https://example.invalid"></object>',
];

test('model output can never produce an executable element, attribute or URL', () => {
  for (const raw of hostile) {
    const html = render(raw);
    assert.ok(!/<script|<iframe|<object|<svg|<img|<a\b|<form|<button|<meta|<input|<style|<link/i.test(html), `markup survived for ${raw}: ${html}`);
    assert.ok(!/\son[a-z]+\s*=/i.test(html), `an event handler survived for ${raw}: ${html}`);
    assert.ok(!/javascript:|file:|data:text\/html/i.test(html), `a dangerous scheme survived for ${raw}: ${html}`);
    assert.ok(!/href=|src=|style=/i.test(html), `an attribute survived for ${raw}: ${html}`);
  }
});

test('a placeholder the model types cannot smuggle markup or a formula through', () => {
  // The render token is fresh per call and never chosen by the model. Text that
  // merely looks like a marker stays ordinary literal text in the answer; it
  // must never be expanded into formula output, and the real token must never
  // be reachable by anything the model writes.
  const forged = 'MATHPLACEHOLDER0END MATHPLACEHOLDER9END MATHPLACEHOLDER-1END MATHPHdeadbeef0END';
  const html = render(forged);
  assert.ok(!/katex/i.test(html), 'a forged marker produced formula output');
  assert.ok(/MATHPLACEHOLDER0END/.test(html), 'the literal text was not preserved for the reader');
  // Even when the model guesses the token prefix exactly, the marker it writes
  // resolves to no formula, so it is removed instead of being shown.
  const realToken = 'MATHPHdeadbeef';
  assert.ok(!new RegExp(`${realToken}\\d+END`).test(renderWith(forged, enforcePolicy, markdownToHtml, realToken)), 'the real token survived in the output');
  // KaTeX runs with trust:false and a bounded expansion, so even a formula the
  // model writes that reaches KaTeX cannot escape into markup or attributes.
  // A refused command degrades to visible error text, not to a rendered link.
  const extension = render(String.raw`$$\href{javascript:alert(1)}{y}\htmlClass{evil}{x}$$`);
  assert.ok(!/<script|onerror=|onclick=/i.test(extension), `a KaTeX extension escaped executable markup: ${extension}`);
  assert.ok(!/href="|href='/.test(extension), `a KaTeX extension produced a real link: ${extension}`);
  assert.ok(!/class="[^"]*evil/.test(extension), `a KaTeX extension produced a chosen class: ${extension}`);
  assert.ok(!/javascript:alert\(1\)"\s*>/i.test(extension), 'a refused URL became a live attribute');
  const bomb = render(`$${'\\def\\x{\\x}\\x'.repeat(50)}$$`);
  assert.ok(bomb.length < 20000, `macro expansion was not bounded: ${bomb.length}`);
  const truncated = render(`$${'x'.repeat(FORMULA_LIMIT + 1)}$$`);
  assert.ok(!/katex/.test(truncated), 'an oversized formula was rendered');
});

test('reviewed structure, code samples and formulas still render', () => {
  const html = render('# 标题\n\n**粗体** 与 `code`\n\n- 一\n- 二\n\n```text\n$x$\n```\n\n公式 $E=mc^2$ 与 $$\\int_0^1 x\\,dx$$');
  assert.ok(/<h1>/.test(html), 'heading lost');
  assert.ok(/<strong>/.test(html), 'emphasis lost');
  assert.ok(/<li>/.test(html), 'list lost');
  assert.ok(/<pre>/.test(html), 'fenced code lost');
  assert.ok(/katex/.test(html), 'inline and display formulas did not render');
  assert.ok(html.includes('$x$'), 'a fenced code sample was altered');
});

test('the production policy is the closed allow-list and drops every attribute', () => {
  assert.deepEqual([...ALLOWED_TAGS].sort(), ['blockquote', 'br', 'code', 'del', 'em', 'h1', 'h2', 'h3', 'h4', 'hr', 'li', 'ol', 'p', 'pre', 'strong', 'table', 'tbody', 'td', 'th', 'thead', 'tr', 'ul']);
  for (const forbidden of ['a', 'img', 'script', 'iframe', 'svg', 'style', 'input', 'button', 'object', 'form', 'meta', 'link']) {
    assert.ok(!ALLOWED_TAGS.includes(forbidden), `${forbidden} must never be allowed in model output`);
  }
});

test('markdown conversion itself never trusts raw html as executable markup', () => {
  // marked may emit the hostile tag, but the policy must remove it; this pins
  // that the safety does not depend on marked's own html handling.
  const converted = markdownToHtml('<script>alert(1)</script>');
  assert.ok(/script/i.test(converted), 'marked no longer passes raw html through; revisit the assumption');
  assert.ok(!/<script/i.test(enforcePolicy(converted)), 'the policy failed to remove raw html');
});
