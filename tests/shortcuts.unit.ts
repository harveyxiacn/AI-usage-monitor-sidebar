import { test, expect } from '@playwright/test';
import { shortcutProblem } from '../src/lib/shortcuts';

test('an empty shortcut setting is valid and simply disables the shortcut', () => {
  expect(shortcutProblem('')).toBeNull();
  expect(shortcutProblem('   ')).toBeNull();
});

test('ordinary combinations are accepted in every spelling Rust accepts', () => {
  for (const value of ['Ctrl+Alt+U', 'ctrl+alt+u', 'CommandOrControl+Shift+D', 'Super+F5', 'Alt + K']) {
    expect(shortcutProblem(value), value).toBeNull();
  }
});

test('shapes that can never register are rejected before the round trip', () => {
  // A bare key would be swallowed in every application on the desktop.
  expect(shortcutProblem('U')).toBe('no-modifier');
  expect(shortcutProblem('F5')).toBe('no-modifier');
  expect(shortcutProblem('Ctrl+')).toBe('empty-token');
  expect(shortcutProblem('Ctrl+Alt')).toBe('no-key');
  expect(shortcutProblem('Ctrl+A+B')).toBe('too-many-keys');
});
