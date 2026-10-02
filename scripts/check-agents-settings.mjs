#!/usr/bin/env node
// Keeps the settings table in AGENTS.md (section 5) in step with the code.
//
// Source of truth: the top-level keys of `defaultSettings` in
// src/lib/settings-defaults.ts, cross-checked against the field list of
// `impl Default for Settings` in src-tauri/src/model.rs (snake_case ->
// camelCase). Both are read as text, so no TypeScript tooling is needed.
//
// Fails (exit 1) when a key has no row, has more than one row, or a row names
// a key that does not exist.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8').replace(/\r\n/g, '\n');

/** Top-level keys of the `defaultSettings` object literal (2-space indent). */
function tsKeys() {
  const src = read('src/lib/settings-defaults.ts');
  const start = src.indexOf('export const defaultSettings');
  if (start < 0) throw new Error('defaultSettings not found in settings-defaults.ts');
  const body = src.slice(src.indexOf('{', start));
  const end = body.indexOf('\n};');
  if (end < 0) throw new Error('end of defaultSettings not found');
  return [...body.slice(0, end).matchAll(/^ {2}([A-Za-z0-9_]+):/gm)].map((m) => m[1]);
}

const camel = (s) => s.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase());

/** Field names initialised in `impl Default for Settings`, camelCased. */
function rustKeys() {
  const src = read('src-tauri/src/model.rs');
  const impl = src.indexOf('impl Default for Settings');
  if (impl < 0) throw new Error('impl Default for Settings not found in model.rs');
  const lit = src.indexOf('\n        Settings {\n', impl);
  if (lit < 0) throw new Error('Settings literal not found in model.rs');
  const end = src.indexOf('\n        }\n', lit);
  return [...src.slice(lit, end).matchAll(/^ {12}([a-z0-9_]+)[:,]/gm)].map((m) => camel(m[1]));
}

/** First-column keys of the AGENTS.md section 5 table. */
function agentsKeys() {
  const md = read('AGENTS.md');
  const start = md.search(/^## 5\./m);
  if (start < 0) throw new Error('"## 5." heading not found in AGENTS.md');
  const rest = md.slice(start + 4);
  const next = rest.search(/^## /m);
  const section = next < 0 ? rest : rest.slice(0, next);
  const keys = [];
  for (const line of section.split('\n')) {
    const m = line.match(/^\|\s*`([A-Za-z0-9_]+)`\s*\|/);
    if (m) keys.push(m[1]);
  }
  return keys;
}

const diff = (a, b) => a.filter((x) => !b.includes(x));
const dupes = (a) => [...new Set(a.filter((x, i) => a.indexOf(x) !== i))];

const ts = tsKeys();
const rs = rustKeys();
const md = agentsKeys();
const errors = [];

for (const k of diff(ts, rs)) errors.push(`key in settings-defaults.ts but not in model.rs Default: ${k}`);
for (const k of diff(rs, ts)) errors.push(`key in model.rs Default but not in settings-defaults.ts: ${k}`);
for (const k of diff(ts, md)) errors.push(`AGENTS.md section 5 has no row for: ${k}`);
for (const k of diff(md, ts)) errors.push(`AGENTS.md section 5 documents a key that does not exist: ${k}`);
for (const k of dupes(md)) errors.push(`AGENTS.md section 5 has more than one row for: ${k}`);

if (errors.length) {
  console.error('check-agents-settings: FAILED');
  for (const e of errors) console.error('  - ' + e);
  process.exit(1);
}
console.log(`check-agents-settings: ok (${ts.length} settings keys, ${md.length} AGENTS.md rows)`);
