import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { formatTokenCount, isTokenUnit } from '../src/lib/tokenFormat.ts';
const cases = JSON.parse(readFileSync(new URL('./token-units.json', import.meta.url)));
test('Chinese and international examples and boundaries', () => {
  for (const row of cases) {
    for (const unit of ['chinese', 'international']) {
      assert.equal(formatTokenCount(row.value, unit), row[unit], `${row.value} ${unit}`);
    }
  }
});
test('fractional rates and invalid values', () => {
  assert.equal(formatTokenCount(12.34, 'chinese'), '12.34');
  assert.equal(formatTokenCount(NaN, 'chinese'), '—');
  assert.equal(isTokenUnit('full'), false);
  assert.equal(isTokenUnit('international'), true);
});
