import { test, expect } from '@playwright/test';
import { rankCommands, scoreCommand, scoreTerm, type PaletteCommand } from '../src/lib/palette';

const cmd = (id: string, title: string, extra: Partial<PaletteCommand> = {}): PaletteCommand => ({
  id,
  group: 'action',
  title,
  run: () => {},
  ...extra,
});

test('a prefix beats a word start, which beats a substring, which beats scattered letters', () => {
  const prefix = scoreTerm('ref', 'refresh now')!;
  const word = scoreTerm('now', 'refresh now')!;
  const inside = scoreTerm('fre', 'refresh now')!;
  const scattered = scoreTerm('rfs', 'refresh now')!;
  expect(prefix).toBeGreaterThan(word);
  expect(word).toBeGreaterThan(inside);
  expect(inside).toBeGreaterThan(scattered);
  expect(scoreTerm('zzz', 'refresh now')).toBeNull();
});

test('two scattered letters are noise, three are a fuzzy match', () => {
  expect(scoreTerm('rn', 'refresh now')).toBeNull();
  expect(scoreTerm('rsn', 'refresh now')).not.toBeNull();
});

test('every term must match; extra keywords count for less than the title', () => {
  const c = cmd('a', 'Pause polling', { keywords: 'stop quota' });
  expect(scoreCommand('pause poll', c)).not.toBeNull();
  expect(scoreCommand('pause nothing', c)).toBeNull();
  expect(scoreCommand('stop', c)).not.toBeNull();
  const inTitle = scoreCommand('quota', cmd('b', 'Quota history'))!;
  const inKeywords = scoreCommand('quota', c)!;
  expect(inTitle).toBeGreaterThan(inKeywords);
});

test('matching ignores case and extra whitespace, and handles CJK by substring', () => {
  expect(scoreCommand('  REFRESH   now ', cmd('a', 'Refresh now'))).not.toBeNull();
  expect(scoreCommand('轮询', cmd('a', '暂停轮询'))).not.toBeNull();
  expect(scoreCommand('刷新', cmd('a', '暂停轮询'))).toBeNull();
});

test('ranking puts the best match first and is stable for ties (group, then registry order)', () => {
  const list = [
    cmd('1', 'Open log folder'),
    cmd('2', 'Go to History', { group: 'navigate' }),
    cmd('3', 'Toggle: Auto-hide', { group: 'setting' }),
    cmd('4', 'History export', { group: 'action' }),
  ];
  expect(rankCommands('hist', list).map((c) => c.id)).toEqual(['4', '2']);
  // equal scores: navigate comes before action before setting
  const ties = [cmd('s', 'Alpha', { group: 'setting' }), cmd('a', 'Alpha', { group: 'action' }), cmd('n', 'Alpha', { group: 'navigate' })];
  expect(rankCommands('alpha', ties).map((c) => c.id)).toEqual(['n', 'a', 's']);
});

test('an empty query lists everything grouped, and the limit caps the list', () => {
  const list = [cmd('s', 'S', { group: 'setting' }), cmd('n', 'N', { group: 'navigate' }), cmd('a', 'A')];
  expect(rankCommands('', list).map((c) => c.id)).toEqual(['n', 'a', 's']);
  expect(rankCommands('', list, 2)).toHaveLength(2);
  expect(rankCommands('   ', list)).toHaveLength(3);
});

test('a query with no match returns nothing', () => {
  expect(rankCommands('qqqq', [cmd('a', 'Refresh now')])).toEqual([]);
});
