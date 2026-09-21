import { test, expect } from '@playwright/test';
import { accountEmail, maskEmail } from '../src/lib/privacy';

test('maskEmail keeps one recognisable character per part and the TLD', () => {
  expect(maskEmail('harvey@gmail.com')).toBe('h•••@g•••.com');
  expect(maskEmail('a@x.com')).toBe('a•••@x•••.com');
  expect(maskEmail('first.last+tag@mail.example.co.uk')).toBe('f•••@m•••.uk');
  // an uppercase address is not normalised — only hidden
  expect(maskEmail('Harvey@Example.COM')).toBe('H•••@E•••.COM');
});

test('maskEmail never leaks a value it does not understand', () => {
  // no domain label separator: the whole domain collapses
  expect(maskEmail('user@localhost')).toBe('u•••@l•••');
  // no local part / no domain at all
  expect(maskEmail('@example.com')).toBe('•••@e•••.com');
  expect(maskEmail('user@')).toBe('u•••@');
  // not an address at all — masked as a single run, never echoed
  expect(maskEmail('not-an-email')).toBe('n•••');
  expect(maskEmail('a@b@c.com')).toBe('a•••@c•••.com');
});

test('maskEmail is empty for empty input and trims first', () => {
  expect(maskEmail(null)).toBe('');
  expect(maskEmail(undefined)).toBe('');
  expect(maskEmail('')).toBe('');
  expect(maskEmail('   ')).toBe('');
  expect(maskEmail('  harvey@gmail.com  ')).toBe('h•••@g•••.com');
});

test('maskEmail keeps a whole code point, not half a surrogate pair', () => {
  expect(maskEmail('张三@例子.中国')).toBe('张•••@例•••.中国');
  expect(maskEmail('😀me@😀.io')).toBe('😀•••@😀•••.io');
});

test('accountEmail masks only when the setting is on', () => {
  expect(accountEmail('harvey@gmail.com', false)).toBe('harvey@gmail.com');
  expect(accountEmail('harvey@gmail.com', true)).toBe('h•••@g•••.com');
  expect(accountEmail(null, true)).toBe('');
  expect(accountEmail(null, false)).toBe('');
});
