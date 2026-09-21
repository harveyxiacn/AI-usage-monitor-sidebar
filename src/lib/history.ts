import type { HistoryRow, ProviderId } from './types';

export type HistoryPreset = 'today' | '7d' | '30d' | '90d' | 'custom';
export interface HistoryRange { from: number; to: number }

/** Both Windows and POSIX paths are displayable on any host platform. */
export function projectName(path: string, unassigned = 'Unassigned'): string {
  // Only the empty string is "unassigned" (contract §4); a blank-looking path
  // is a real, distinct project, so it is quoted rather than rendered blank.
  if (path === '') return unassigned;
  const withoutTrailingSeparator = path.replace(/[\\/]+$/, '');
  const name = withoutTrailingSeparator.split(/[\\/]/).at(-1) || path;
  return name.trim() === '' ? `"${name}"` : name;
}

/** Keep chart legends and tooltips readable when a project path is long. */
export function shortenLabel(text: string, max = 36): string {
  if (text.length <= max) return text;
  const head = Math.ceil((max - 1) / 2);
  return `${text.slice(0, head)}…${text.slice(text.length - (max - 1 - head))}`;
}

/** Keep short names when unique; show their parents when basenames collide. */
export function projectLabels(projects: readonly string[], unassigned = 'Unassigned'): Map<string, string> {
  const unique = [...new Set(projects)];
  const names = unique.map((path) => projectName(path, unassigned));
  const uses = new Map<string, number>();
  for (const name of names) uses.set(name, (uses.get(name) ?? 0) + 1);
  return new Map(unique.map((path, index) => {
    const name = names[index];
    if (path === '' || uses.get(name)! < 2) return [path, name];
    const trimmed = path.replace(/[\\/]+$/, '');
    const separator = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
    const parent = trimmed.slice(0, separator) || path;
    return [path, `${name} — ${parent}`];
  }));
}

export interface HistorySeries {
  key: string;
  provider: ProviderId;
  model: string | null;
  project: string | null;
  values: Array<number | null>;
}

/** Stable, collision-free identities keep similarly named models/projects apart. */
export function historySeries(rows: readonly HistoryRow[], groupByModel: boolean, groupByProject: boolean, metric: 'tokens' | 'cost') {
  const buckets = [...new Set(rows.map((row) => row.bucketStart))].sort((a, b) => Date.parse(a) - Date.parse(b));
  const indices = new Map(buckets.map((bucket, index) => [bucket, index]));
  const series = new Map<string, HistorySeries>();
  for (const row of rows) {
    const model = groupByModel ? row.model : null;
    const project = groupByProject ? row.project ?? '' : null;
    const key = JSON.stringify([row.provider, model, project]);
    let entry = series.get(key);
    if (!entry) {
      entry = { key, provider: row.provider, model, project, values: new Array(buckets.length).fill(0) };
      series.set(key, entry);
    }
    const index = indices.get(row.bucketStart)!;
    const value = metric === 'cost' ? row.estimatedCostUsd : row.totalTokens;
    const previous = entry.values[index];
    entry.values[index] = value == null || previous == null ? null : previous + value;
  }
  return { buckets, series: [...series.values()].sort((a, b) => a.key.localeCompare(b.key)) };
}

export function localDateInput(ms: number): string {
  const date = new Date(ms);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function parseLocalDate(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const date = new Date(`${value}T00:00:00`);
  return Number.isFinite(date.getTime()) && localDateInput(date.getTime()) === value ? date : null;
}

/** Calendar days, including today. The upper bound is always exclusive. */
export function historyRange(preset: HistoryPreset, from: string, to: string, now = Date.now()): HistoryRange | null {
  if (preset === 'custom') {
    const first = parseLocalDate(from);
    const last = parseLocalDate(to);
    if (!first || !last || last < first) return null;
    // Calendar arithmetic preserves 23/25-hour days across DST transitions.
    last.setDate(last.getDate() + 1);
    return { from: first.getTime(), to: last.getTime() };
  }
  const first = new Date(now);
  first.setHours(0, 0, 0, 0);
  const days = preset === 'today' ? 1 : Number.parseInt(preset, 10);
  first.setDate(first.getDate() - days + 1);
  return { from: first.getTime(), to: now };
}

/** Escape spreadsheet formula prefixes as well as RFC 4180 delimiters. */
export function csvCell(value: string | number): string {
  let text = String(value);
  if (typeof value === 'string' && /^[\s\u0000-\u001f]*[=+@-]/.test(text)) text = `'${text}`;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export function historyCsv(rows: readonly HistoryRow[]): string {
  const columns: Array<[string, (row: HistoryRow) => string | number]> = [
    ['bucket_start', (r) => r.bucketStart],
    ['provider', (r) => r.provider],
    ['model', (r) => r.model ?? ''],
    ['project', (r) => r.project ?? ''],
    ['input_tokens', (r) => r.inputTokens],
    ['cache_write_tokens', (r) => r.cacheWriteTokens],
    ['cache_read_tokens', (r) => r.cacheReadTokens],
    ['output_tokens', (r) => r.outputTokens],
    ['reasoning_tokens', (r) => r.reasoningTokens],
    ['total_tokens', (r) => r.totalTokens],
    ['requests', (r) => r.requests],
    ['estimated_cost_usd', (r) => r.estimatedCostUsd == null ? '' : r.estimatedCostUsd.toFixed(4)],
  ];
  return [columns.map(([name]) => name).join(','), ...rows.map((row) => columns.map(([, get]) => csvCell(get(row))).join(','))].join('\r\n') + '\r\n';
}
