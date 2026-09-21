import { test, expect } from '@playwright/test';
import { mergeSettings, SettingsWriter, type SettingsPatch } from '../src/lib/settings-writer';
import { mockSettings } from '../src/lib/mock';
import type { Settings } from '../src/lib/types';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test('rapid settings edits remain visible while previous saves complete', async () => {
  const first = deferred<Settings>();
  const second = deferred<Settings>();
  const calls: SettingsPatch[] = [];
  let visible = structuredClone(mockSettings);
  let pending = false;
  const writer = new SettingsWriter(visible, (patch) => {
    calls.push(patch);
    return calls.length === 1 ? first.promise : second.promise;
  }, (value, saving) => { visible = value; pending = saving; }, (error) => { throw error; });
  const a = writer.patch({ scale: 1.1 });
  const b = writer.patch({ scale: 1.4, colors: { claude: '#112233' } });
  expect(visible.scale).toBe(1.4);
  expect(calls).toHaveLength(1);
  first.resolve(mergeSettings(mockSettings, { scale: 1.1 }));
  await a;
  expect(visible.scale).toBe(1.4);
  expect(visible.colors.codex).toBe(mockSettings.colors.codex);
  expect(pending).toBe(true);
  second.resolve(mergeSettings(mockSettings, { scale: 1.4, colors: { claude: '#112233' } }));
  await b;
  expect(pending).toBe(false);
  expect(visible.colors.claude).toBe('#112233');
});

test('failed saves roll back only their own edit and preserve newer queued edits', async () => {
  const first = deferred<Settings>();
  const errors: unknown[] = [];
  let visible = structuredClone(mockSettings);
  let call = 0;
  const writer = new SettingsWriter(visible, async (patch) => {
    if (++call === 1) return first.promise;
    return mergeSettings(mockSettings, patch);
  }, (value) => { visible = value; }, (error) => errors.push(error));
  const a = writer.patch({ theme: 'light' });
  const b = writer.patch({ edge: 'left', sizes: { ringSize: 72 } });
  first.reject(new Error('disk full'));
  await Promise.all([a, b]);
  expect(visible.theme).toBe('dark');
  expect(visible.edge).toBe('left');
  expect(visible.sizes.ringSize).toBe(72);
  expect(errors).toHaveLength(1);
});

test('external window events survive a stale save response', async () => {
  const save = deferred<Settings>();
  let visible = structuredClone(mockSettings);
  const writer = new SettingsWriter(visible, () => save.promise, (value) => { visible = value; }, (error) => { throw error; });
  const done = writer.patch({ theme: 'light' });
  writer.receive(mergeSettings(mockSettings, { theme: 'light', edge: 'left' }));
  save.resolve(mergeSettings(mockSettings, { theme: 'light' }));
  await done;
  expect(visible.edge).toBe('left');
  expect(visible.theme).toBe('light');
});
