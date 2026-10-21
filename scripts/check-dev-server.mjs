// HTTP boundary checks only; never a substitute for real browser interaction.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import http from 'node:http';
const base = 'http://127.0.0.1:43182';
const results = [];
const request = (route, options = {}) => fetch(base + route, { ...options, redirect: 'manual' });
const index = await request('/');
assert.equal(index.status, 200);
assert.match(await index.text(), /AI-Native PDF Reader/);
const csp = index.headers.get('content-security-policy');
assert.ok(csp?.includes("object-src 'none'") && csp.includes("script-src 'self' 'wasm-unsafe-eval'"));
assert.equal(index.headers.get('access-control-allow-origin'), null);
results.push({ check: 'index, CSP, no cross-origin access', pass: true });
for (const route of ['/src/main.ts', '/src/App.svelte', '/pdfjs/version.json', '/pdfjs/standard_fonts/LiberationSans-Regular.ttf']) {
  assert.equal((await request(route)).status, 200, route);
  results.push({ check: route, pass: true });
}
const sample = await request('/__samples/geometry.pdf');
assert.equal(sample.status, 200);
assert.equal(createHash('sha256').update(Buffer.from(await sample.arrayBuffer())).digest('hex'),
  '1917dff0058a73a311bc820623a68b1c36edbaf1a394b5782e7df4092df1484f');
results.push({ check: 'fixed approved development sample', pass: true });
for (const route of ['/PRODUCT_SPEC.md', '/.git/config', '/tests/fixtures/g1/geometry.pdf', '/spikes/g1/package.json',
  '/@fs/D:/Projects/ai-native-pdf-reader/PRODUCT_SPEC.md', '/@fs/D:/Projects/ai-native-pdf-reader/package-lock.json',
  '/__samples/plain.pdf', '/__samples/%2e%2e%2fplain.pdf']) {
  const response = await request(route);
  assert.ok(response.status === 403 || response.status === 404, `${route}: ${response.status}`);
  results.push({ check: route, status: response.status, pass: true });
}
assert.equal((await request('/__samples/geometry.pdf', { method: 'POST' })).status, 404);
results.push({ check: 'sample rejects POST', pass: true });
// Fetch rewrites Host; use an explicit HTTP request for this boundary test.
const foreignStatus = await new Promise((resolve, reject) => {
  const probe = http.get(base + '/', { headers: { host: 'untrusted.example' } }, response => {
    response.resume(); resolve(response.statusCode);
  });
  probe.on('error', reject); probe.setTimeout(5000, () => probe.destroy(new Error('Probe timed out')));
});
assert.equal(foreignStatus, 403);
results.push({ check: 'unapproved Host', status: foreignStatus, pass: true });
console.log(JSON.stringify({ kind: 'HTTP only, not GUI acceptance', results }, null, 2));
