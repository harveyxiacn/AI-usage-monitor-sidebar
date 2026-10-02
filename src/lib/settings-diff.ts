// "What would change / what changed" between two settings, as a flat list.
// Used by the import result, the preset preview and the undo history. [FRONTEND]
import type { Settings } from './types';

export interface SettingChange {
  /** dotted path, e.g. `edge`, `colors.claude`, `providers.codex.enabled` */
  path: string;
  before: unknown;
  after: unknown;
}

/** The schema version is noise in a change list. */
const SKIP = new Set(['version']);
/** Whole-value leaves: shown as one change instead of being flattened. */
const LEAVES = new Set(['customPresets']);

const isObject = (v: unknown): v is Record<string, unknown> =>
  v !== null && typeof v === 'object' && !Array.isArray(v);

function walk(path: string, before: unknown, after: unknown, out: SettingChange[]): void {
  if (isObject(before) && isObject(after) && !LEAVES.has(path)) {
    const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
    for (const key of [...keys].sort()) {
      const child = path === '' ? key : `${path}.${key}`;
      if (path === '' && SKIP.has(key)) continue;
      walk(child, before[key], after[key], out);
    }
    return;
  }
  if (JSON.stringify(before) !== JSON.stringify(after)) out.push({ path, before, after });
}

export function diffSettings(before: Settings, after: Settings): SettingChange[] {
  const out: SettingChange[] = [];
  walk('', before, after, out);
  return out;
}

/** Short, language-neutral rendering of one value for a change list. */
export function formatSettingValue(value: unknown): string {
  if (value === null || value === undefined) return '—';
  if (value === '') return '""';
  if (typeof value === 'boolean') return value ? 'on' : 'off';
  if (isObject(value)) return `{${Object.keys(value).length}}`;
  if (Array.isArray(value)) return `[${value.length}]`;
  return String(value);
}
