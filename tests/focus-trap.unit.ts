import { test, expect } from '@playwright/test';
import { nextTabStop } from '../src/lib/focus-trap';

test('Tab wraps from the last stop to the first, Shift+Tab from the first to the last', () => {
  expect(nextTabStop(2, 3, false)).toBe(0);
  expect(nextTabStop(0, 3, true)).toBe(2);
});

test('Tab in the middle is left to the browser', () => {
  expect(nextTabStop(1, 3, false)).toBeNull();
  expect(nextTabStop(1, 3, true)).toBeNull();
});

test('focus outside the dialog is pulled in', () => {
  expect(nextTabStop(-1, 3, false)).toBe(0);
  expect(nextTabStop(-1, 3, true)).toBe(2);
});

test('a single stop keeps focus on itself and no stops is not handled', () => {
  expect(nextTabStop(0, 1, false)).toBe(0);
  expect(nextTabStop(0, 1, true)).toBe(0);
  expect(nextTabStop(0, 0, false)).toBeNull();
});
