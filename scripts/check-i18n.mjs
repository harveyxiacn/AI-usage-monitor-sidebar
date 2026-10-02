#!/usr/bin/env node
// Keeps the two UI catalogues honest. Run with `pnpm check:i18n` (also in CI).
//
//   1. en.json and zh-CN.json have exactly the same keys;
//   2. no value is empty or whitespace only;
//   3. a value uses the same {placeholders} in both languages;
//   4. every literal key passed to t('…') / tDyn('…') somewhere in src exists.
//
// Keys assembled at run time (t(`settings.theme.${v}`)) cannot be checked
// statically; those go through tDyn() on purpose, and their catalogue entries
// are covered by rules 1-3.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const i18nDir = path.join(root, 'src/lib/i18n');
const load = (name) => JSON.parse(readFileSync(path.join(i18nDir, `${name}.json`), 'utf8'));
const en = load('en');
const zh = load('zh-CN');

const problems = [];

for (const key of Object.keys(en)) {
  if (!(key in zh)) problems.push(`zh-CN.json is missing "${key}"`);
}
for (const key of Object.keys(zh)) {
  if (!(key in en)) problems.push(`en.json is missing "${key}" (only in zh-CN.json)`);
}
for (const [name, catalogue] of [['en.json', en], ['zh-CN.json', zh]]) {
  for (const [key, value] of Object.entries(catalogue)) {
    if (typeof value !== 'string' || value.trim() === '') problems.push(`${name}: "${key}" is empty`);
  }
}
const placeholders = (text) => [...String(text).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
for (const key of Object.keys(en)) {
  if (key in zh && placeholders(en[key]) !== placeholders(zh[key])) {
    problems.push(`"${key}" uses different placeholders: en {${placeholders(en[key])}} vs zh-CN {${placeholders(zh[key])}}`);
  }
}

/** All .svelte / .ts files under `dir`. */
function* sources(dir) {
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) yield* sources(full);
    else if (/\.(svelte|ts)$/.test(entry)) yield full;
  }
}

// `t(` or `tDyn(` that is not part of a longer name (st(, .t(, format(, …)
const CALL = /(?<![\w$.])(?:t|tDyn)\(\s*(['"])([^'"\n]+)\1/g;
let used = 0;
for (const file of sources(path.join(root, 'src'))) {
  const text = readFileSync(file, 'utf8');
  for (const match of text.matchAll(CALL)) {
    used += 1;
    const key = match[2];
    if (!(key in en)) {
      const line = text.slice(0, match.index).split('\n').length;
      problems.push(`${path.relative(root, file)}:${line} uses unknown key "${key}"`);
    }
  }
}

if (problems.length > 0) {
  console.error(`i18n check failed (${problems.length}):`);
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}
console.log(`i18n ok: ${Object.keys(en).length} keys in both languages, ${used} literal lookups resolved.`);
