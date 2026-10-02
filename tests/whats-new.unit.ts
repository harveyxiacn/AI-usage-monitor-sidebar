import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { parseInline, parseMarkdown, safeHref } from '../src/lib/markdown';
import {
  compareVersions,
  localeOf,
  notesFor,
  releaseNotesKey,
  shouldShowWhatsNew,
  splitReleaseNotes,
} from '../src/lib/whats-new';

const SAMPLE = `# AI Usage Sidebar v0.6.0

## 中文

- 新增 **专注模式**，见 [说明](../DOC.md)。

## English

- Added **focus mode**, see [docs](https://example.com/focus).
- Second item with \`code\`.

Closing paragraph
that wraps.
`;

test('versions compare numerically and ignore a leading v', () => {
  expect(compareVersions('0.10.0', '0.9.0')).toBeGreaterThan(0);
  expect(compareVersions('v1.0.0', '1.0.0')).toBe(0);
  expect(compareVersions('0.5.0-rc.1', '0.5.0')).toBe(0);
  expect(compareVersions('dev', '0.5.0')).toBeNaN();
});

test('the notes file is found by exact version only', () => {
  const paths = ['../../docs/releases/v0.5.0.md', '../../docs/releases/v0.6.0.md'];
  expect(releaseNotesKey(paths, '0.6.0')).toBe('../../docs/releases/v0.6.0.md');
  expect(releaseNotesKey(paths, '0.6.0-mock')).toBe('../../docs/releases/v0.6.0.md');
  expect(releaseNotesKey(paths, '0.7.0')).toBeNull();
  expect(releaseNotesKey(paths, 'garbage')).toBeNull();
});

test('a release file splits into its two language sections', () => {
  const notes = splitReleaseNotes(SAMPLE);
  expect(notes.title).toBe('AI Usage Sidebar v0.6.0');
  expect(notes.zh).toContain('专注模式');
  expect(notes.zh).not.toContain('Added');
  expect(notes.en).toContain('Added');
  expect(notesFor(notes, 'zh-CN')).toBe(notes.zh);
  expect(notesFor(notes, 'en')).toBe(notes.en);
  expect(notesFor({ title: '', zh: '', en: 'only english' }, 'zh-CN')).toBe('only english');
});

test('the real v0.5.0 notes have both sections', () => {
  const notes = splitReleaseNotes(readFileSync('docs/releases/v0.5.0.md', 'utf8'));
  expect(notes.zh.length).toBeGreaterThan(50);
  expect(notes.en.length).toBeGreaterThan(50);
});

test('the panel appears once per version, never during the first-run wizard', () => {
  const base = { onboarded: true, lastSeenVersion: '0.5.0', currentVersion: '0.6.0', hasNotes: true };
  expect(shouldShowWhatsNew(base)).toBe(true);
  expect(shouldShowWhatsNew({ ...base, lastSeenVersion: '0.6.0' })).toBe(false);
  expect(shouldShowWhatsNew({ ...base, lastSeenVersion: '' })).toBe(true);
  expect(shouldShowWhatsNew({ ...base, onboarded: false })).toBe(false);
  expect(shouldShowWhatsNew({ ...base, hasNotes: false })).toBe(false);
  expect(shouldShowWhatsNew({ ...base, currentVersion: '' })).toBe(false);
});

test('the language setting picks the notes locale', () => {
  expect(localeOf('en', 'zh-CN')).toBe('en');
  expect(localeOf('zh-CN', 'en-US')).toBe('zh-CN');
  expect(localeOf('auto', 'zh-TW')).toBe('zh-CN');
  expect(localeOf('auto', 'de-DE')).toBe('en');
});

test('markdown becomes a data tree of headings, lists and paragraphs', () => {
  const blocks = parseMarkdown(SAMPLE.split('## English')[1]);
  expect(blocks.map((b) => b.type)).toEqual(['list', 'paragraph']);
  const list = blocks[0];
  if (list.type !== 'list') throw new Error('expected a list');
  expect(list.items).toHaveLength(2);
  expect(list.items[0]).toEqual([
    { type: 'text', text: 'Added ' },
    { type: 'strong', text: 'focus mode' },
    { type: 'text', text: ', see ' },
    { type: 'link', text: 'docs', href: 'https://example.com/focus' },
    { type: 'text', text: '.' },
  ]);
  const paragraph = blocks[1];
  if (paragraph.type !== 'paragraph') throw new Error('expected a paragraph');
  expect(paragraph.inlines).toEqual([{ type: 'text', text: 'Closing paragraph that wraps.' }]);
  expect(parseMarkdown('## Title\n\ntext')[0]).toEqual({ type: 'heading', level: 2, inlines: [{ type: 'text', text: 'Title' }] });
});

test('only http(s) links survive; everything else is plain text', () => {
  expect(safeHref('https://example.com/a?b=1')).toBe('https://example.com/a?b=1');
  expect(safeHref('javascript:alert(1)')).toBeNull();
  expect(safeHref('data:text/html,<script>alert(1)</script>')).toBeNull();
  expect(safeHref('../SESSIONS.md')).toBeNull();
  expect(parseInline('[x](javascript:void)')).toEqual([{ type: 'text', text: 'x' }]);
  expect(parseInline('[guide](../SESSIONS.md)')).toEqual([{ type: 'text', text: 'guide' }]);
});

test('raw HTML in a note stays literal text, never markup', () => {
  const blocks = parseMarkdown('- <img src=x onerror=alert(1)> and <b>bold</b>');
  const serialised = JSON.stringify(blocks);
  expect(blocks[0].type).toBe('list');
  // every inline is text; the angle brackets are data, not elements
  expect(serialised).not.toContain('"type":"html"');
  expect(serialised).toContain('<img src=x onerror=alert(1)>');
});
