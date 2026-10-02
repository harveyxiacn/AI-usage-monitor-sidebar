import { test, expect } from '@playwright/test';
import { keyName, recordKey, type KeyInput } from '../src/lib/shortcut-recorder';
import { shortcutProblem } from '../src/lib/shortcuts';

const press = (code: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  key: code,
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

test('modifiers plus a letter become the accelerator the settings file stores', () => {
  expect(recordKey(press('KeyU', { ctrlKey: true, altKey: true }))).toEqual({ kind: 'accelerator', value: 'Ctrl+Alt+U' });
  expect(recordKey(press('KeyD', { shiftKey: true, metaKey: true }))).toEqual({ kind: 'accelerator', value: 'Shift+Super+D' });
  expect(recordKey(press('F5', { ctrlKey: true }))).toEqual({ kind: 'accelerator', value: 'Ctrl+F5' });
  expect(recordKey(press('Digit7', { altKey: true }))).toEqual({ kind: 'accelerator', value: 'Alt+7' });
  expect(recordKey(press('ArrowLeft', { ctrlKey: true, shiftKey: true }))).toEqual({ kind: 'accelerator', value: 'Ctrl+Shift+Left' });
});

test('the physical key decides, not the layout or the shift state', () => {
  // Shift+1 types "!" but is still Digit1; an AZERTY "a" is still KeyQ.
  expect(recordKey({ ...press('Digit1', { ctrlKey: true, shiftKey: true }), key: '!' })).toEqual({ kind: 'accelerator', value: 'Ctrl+Shift+1' });
  expect(recordKey({ ...press('KeyQ', { ctrlKey: true }), key: 'a' })).toEqual({ kind: 'accelerator', value: 'Ctrl+Q' });
});

test('Escape cancels and a lone Backspace clears', () => {
  expect(recordKey(press('Escape'))).toEqual({ kind: 'cancel' });
  expect(recordKey(press('Backspace'))).toEqual({ kind: 'clear' });
  // with a modifier they are ordinary (if unbindable) keys, not commands
  expect(recordKey(press('Escape', { ctrlKey: true })).kind).toBe('pending');
  expect(recordKey(press('Backspace', { ctrlKey: true })).kind).toBe('pending');
});

test('a modifier alone keeps listening and reports what is held', () => {
  expect(recordKey(press('ControlLeft', { ctrlKey: true }))).toEqual({ kind: 'pending', modifiers: ['Ctrl'] });
  expect(recordKey(press('ShiftRight', { ctrlKey: true, shiftKey: true }))).toEqual({ kind: 'pending', modifiers: ['Ctrl', 'Shift'] });
  expect(recordKey(press('MetaLeft', { metaKey: true }))).toEqual({ kind: 'pending', modifiers: ['Super'] });
});

test('a bare key or an unbindable key is refused with a reason', () => {
  expect(recordKey(press('KeyU'))).toEqual({ kind: 'pending', modifiers: [], reason: 'no-modifier' });
  expect(recordKey(press('F5'))).toEqual({ kind: 'pending', modifiers: [], reason: 'no-modifier' });
  expect(recordKey(press('Minus', { ctrlKey: true }))).toEqual({ kind: 'pending', modifiers: ['Ctrl'], reason: 'unsupported-key' });
  expect(keyName('F25')).toBeNull();
  expect(keyName('F24')).toBe('F24');
});

test('everything the recorder can produce passes the settings-field validator', () => {
  const codes = ['KeyA', 'KeyZ', 'Digit0', 'Numpad5', 'F1', 'F12', 'F24', 'Space', 'Enter', 'Tab', 'ArrowUp', 'Home', 'PageDown', 'Insert', 'Delete'];
  for (const code of codes) {
    const result = recordKey(press(code, { ctrlKey: true, altKey: true }));
    expect(result.kind, code).toBe('accelerator');
    if (result.kind === 'accelerator') expect(shortcutProblem(result.value), result.value).toBeNull();
  }
});
