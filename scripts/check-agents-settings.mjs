#!/usr/bin/env node
// Keeps the settings table in AGENTS.md (section 5) in step with the code.
//
// Source of truth: the top-level keys of `defaultSettings` in
// src/lib/settings-defaults.ts, cross-checked against the field list of
// `impl Default for Settings` in src-tauri/src/model.rs (snake_case ->
// camelCase). Both are read as text, so no TypeScript tooling is needed.
//
// The last column of each row is the setting's tier (basic / advanced /
// internal), cross-checked against SETTING_TIERS in src/lib/settings-tiers.ts.
//
// Fails (exit 1) when a key has no row, has more than one row, a row names a
// key that does not exist, or a row's tier disagrees with settings-tiers.ts.
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

/** key -> tier of the SETTING_TIERS table in settings-tiers.ts. */
function tsTiers() {
  const src = read('src/lib/settings-tiers.ts');
  const start = src.indexOf('export const SETTING_TIERS');
  if (start < 0) throw new Error('SETTING_TIERS not found in settings-tiers.ts');
  const body = src.slice(src.indexOf('{', src.indexOf('=', start)));
  const end = body.indexOf('\n};');
  if (end < 0) throw new Error('end of SETTING_TIERS not found');
  return Object.fromEntries(
    [...body.slice(0, end).matchAll(/^ {2}([A-Za-z0-9_]+): '([a-z]+)',/gm)].map((m) => [m[1], m[2]]),
  );
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

/** Rows of the AGENTS.md section 5 tables: first-column key and last-column tier. */
function agentsRows() {
  const md = read('AGENTS.md');
  const start = md.search(/^## 5\./m);
  if (start < 0) throw new Error('"## 5." heading not found in AGENTS.md');
  const rest = md.slice(start + 4);
  const next = rest.search(/^## /m);
  const section = next < 0 ? rest : rest.slice(0, next);
  const rows = [];
  for (const line of section.split('\n')) {
    const m = line.match(/^\|\s*`([A-Za-z0-9_]+)`\s*\|/);
    if (!m) continue;
    const cells = line.replace(/\s*\|\s*$/, '').split('|');
    rows.push({ key: m[1], tier: cells[cells.length - 1].trim() });
  }
  return rows;
}

const diff = (a, b) => a.filter((x) => !b.includes(x));
const dupes = (a) => [...new Set(a.filter((x, i) => a.indexOf(x) !== i))];

const ts = tsKeys();
const rs = rustKeys();
const rows = agentsRows();
const md = rows.map((r) => r.key);
const tiers = tsTiers();
const errors = [];

for (const k of diff(ts, rs)) errors.push(`key in settings-defaults.ts but not in model.rs Default: ${k}`);
for (const k of diff(rs, ts)) errors.push(`key in model.rs Default but not in settings-defaults.ts: ${k}`);
for (const k of diff(ts, md)) errors.push(`AGENTS.md section 5 has no row for: ${k}`);
for (const k of diff(md, ts)) errors.push(`AGENTS.md section 5 documents a key that does not exist: ${k}`);
for (const k of dupes(md)) errors.push(`AGENTS.md section 5 has more than one row for: ${k}`);
for (const k of diff(ts, Object.keys(tiers))) errors.push(`settings-tiers.ts has no tier for: ${k}`);
for (const k of diff(Object.keys(tiers), ts)) errors.push(`settings-tiers.ts names a key that does not exist: ${k}`);
for (const { key, tier } of rows) {
  if (!['basic', 'advanced', 'internal'].includes(tier)) {
    errors.push(`AGENTS.md row ${key} has no valid Tier cell (got "${tier}")`);
  } else if (tiers[key] && tiers[key] !== tier) {
    errors.push(`AGENTS.md says ${key} is ${tier}, settings-tiers.ts says ${tiers[key]}`);
  }
}

if (errors.length) {
  console.error('check-agents-settings: FAILED');
  for (const e of errors) console.error('  - ' + e);
  process.exit(1);
}
console.log(`check-agents-settings: ok (${ts.length} settings keys, ${md.length} AGENTS.md rows, tiers match)`);
