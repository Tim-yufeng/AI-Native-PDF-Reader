import test from 'node:test';
import assert from 'node:assert/strict';
import { DocumentTabs } from '../../src/lib/document-tabs.ts';
import { validPosition } from '../../src/lib/reading-position.ts';
const source = { name: 'same.pdf', size: 10, read: async () => new Uint8Array(10) };
test('duplicate and same-named inputs remain independent; closing drops sources', () => {
  const tabs = new DocumentTabs(); tabs.add([source, source]);
  assert.equal(tabs.tabs.length, 2); assert.notEqual(tabs.tabs[0].id, tabs.tabs[1].id);
  const second = tabs.activeId!, first = tabs.tabs[0].id;
  tabs.remember({ page: 27, zoom: 1.2, top: .3, left: 0 }); tabs.activate(first);
  const initial = tabs.active?.position; assert.equal(initial, undefined);
  tabs.remember({ page: 3, zoom: 'page-fit', top: .1, left: 0 }); tabs.activate(second);
  assert.equal(tabs.active?.position?.page, 27);
  assert.equal(tabs.close(first), false); assert.equal(tabs.activeId, second);
  assert.equal(tabs.close(second), true); assert.equal(tabs.activeId, null); assert.equal(tabs.tabs.length, 0);
});
test('reading positions reject corrupted or unbounded values', () => {
  assert.ok(validPosition({ page: 2000, zoom: 'page-fit', top: .5, left: 0 }));
  for (const value of [null, {}, {page: 0}, {page: 1, zoom: NaN, top: 0, left: 0}, {page: 1, zoom: 1, top: Infinity, left: 0}]) assert.equal(validPosition(value), false);
});

