// The single sanitization boundary for remote AI text. The renderer and the
// sanitizer are injected so the guard can be tested without a window, a model
// or a Provider, while production always passes DOMPurify with the policy
// below. The remote answer is untrusted input: it may only produce the reviewed
// structural elements, never an attribute, link, image or executable element.
import DOMPurify from 'dompurify';
import { marked } from 'marked';
import katex from 'katex';
import { splitMath } from './ai-math.ts';

export const ALLOWED_TAGS = ['p', 'br', 'strong', 'em', 'del', 'pre', 'code', 'ul', 'ol', 'li', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'hr'];
export const FORMULA_LIMIT = 4096;

export function markdownToHtml(source: string): string {
  return marked.parse(source, { async: false }) as string;
}

export function sanitizeWithPolicy(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR: [] });
}

export function renderWith(text: string, sanitize: (html: string) => string, toHtml: (source: string) => string = markdownToHtml, token = `MATHPH${crypto.randomUUID().replaceAll('-', '')}`): string {
  const { source, formulas } = splitMath(text, token);
  const safe = sanitize(toHtml(source));
  // The token is fresh for this render, so a model that types something that
  // looks like a marker cannot name a formula or strand a marker in the output.
  return safe.replace(new RegExp(`${token}(\\d+)END`, 'g'), (_raw, index) => {
    const formula = formulas[Number(index)];
    // A marker with no formula behind it is removed rather than shown.
    if (!formula || formula.value.length > FORMULA_LIMIT) return '';
    try {
      return katex.renderToString(formula.value, { displayMode: formula.display, throwOnError: false, trust: false, strict: 'error', maxExpand: 100, maxSize: 10 });
    } catch {
      // A malformed formula degrades to nothing visible, never to raw text.
      return '';
    }
  });
}

export function renderAiAnswer(text: string): string {
  return renderWith(text, sanitizeWithPolicy);
}
