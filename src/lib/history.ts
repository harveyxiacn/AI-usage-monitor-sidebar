import type { CalendarDay, HistoryRow, ProviderId, SessionRow, TokenTotals } from './types';

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

// ------------------------------------------------- activity heatmap -------

export type HeatMetric = 'tokens' | 'cost';

/** The value a heat cell encodes; null = "cost unknown", not "zero". */
export function metricOf(totals: Pick<TokenTotals, 'totalTokens' | 'estimatedCostUsd'>, metric: HeatMetric): number | null {
  return metric === 'cost' ? totals.estimatedCostUsd : totals.totalTokens;
}

/**
 * The GitHub-style grid for `[from, to)`: one entry per week (Monday first),
 * each week seven local `YYYY-MM-DD` slots with `null` where the day falls
 * outside the range — an empty *day* and a *missing* day are different things.
 *
 * Every step is calendar arithmetic re-anchored to local midnight, so a 23- or
 * 25-hour DST day still produces exactly one cell.
 */
export function calendarWeeks(from: number, to: number): Array<Array<string | null>> {
  if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) return [];
  const first = new Date(from);
  first.setHours(0, 0, 0, 0);
  const last = new Date(to - 1);
  last.setHours(0, 0, 0, 0);
  const cursor = new Date(first);
  cursor.setDate(cursor.getDate() - ((cursor.getDay() + 6) % 7));
  cursor.setHours(0, 0, 0, 0);
  const weeks: Array<Array<string | null>> = [];
  while (cursor.getTime() <= last.getTime()) {
    const week: Array<string | null> = [];
    for (let day = 0; day < 7; day += 1) {
      const inside = cursor.getTime() >= first.getTime() && cursor.getTime() <= last.getTime();
      week.push(inside ? localDateInput(cursor.getTime()) : null);
      cursor.setDate(cursor.getDate() + 1);
      // a DST shift at midnight would otherwise drag the cursor off the day
      cursor.setHours(0, 0, 0, 0);
    }
    weeks.push(week);
  }
  return weeks;
}

/**
 * Quartile cut-offs over the non-zero values, so one huge day cannot flatten
 * the rest of the grid into a single shade. The largest value always reaches
 * the darkest level.
 */
export function heatThresholds(values: readonly number[]): [number, number, number] {
  const positive = values.filter((v) => Number.isFinite(v) && v > 0).sort((a, b) => a - b);
  if (positive.length === 0) return [0, 0, 0];
  const at = (q: number) => positive[Math.min(positive.length - 1, Math.floor(q * positive.length))];
  return [at(0.25), at(0.5), at(0.75)];
}

/** 0 = no activity, 1..4 = the four steps of the sequential ramp. */
export function heatLevel(value: number | null, thresholds: readonly number[]): 0 | 1 | 2 | 3 | 4 {
  if (value == null || !(value > 0)) return 0;
  if (value < thresholds[0]) return 1;
  if (value < thresholds[1]) return 2;
  if (value < thresholds[2]) return 3;
  return 4;
}

// --------------------------------------------------- monthly budget -------

export interface BudgetProgress {
  /** local month boundaries, `[monthStart, monthEnd)` */
  monthStart: number;
  monthEnd: number;
  /** estimated cost recorded this month so far */
  spentUsd: number;
  /** at least one day this month contains an unpriced model */
  incomplete: boolean;
  /** share of the month already elapsed, 0..1 */
  elapsed: number;
  percentUsed: number;
  /** linear extrapolation of `spentUsd` to the whole month */
  paceUsd: number;
  /** cumulative spend per elapsed local day, for the burn-up line */
  series: Array<{ date: string; cumulativeUsd: number }>;
}

/**
 * Month-to-date burn-up against `budgetUsd`, from the calendar's daily costs.
 * Returns null when no budget is configured. Month boundaries are local and
 * DST-safe (a 743- or 745-hour month still measures as one month).
 */
export function budgetProgress(
  days: readonly CalendarDay[],
  budgetUsd: number,
  now = Date.now()
): BudgetProgress | null {
  if (!Number.isFinite(budgetUsd) || budgetUsd <= 0) return null;
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  start.setDate(1);
  const end = new Date(start);
  end.setMonth(end.getMonth() + 1);
  const costs = new Map(days.map((day) => [day.date, day.estimatedCostUsd]));

  const series: Array<{ date: string; cumulativeUsd: number }> = [];
  let spentUsd = 0;
  let incomplete = false;
  const cursor = new Date(start);
  while (cursor.getTime() <= now && cursor.getTime() < end.getTime()) {
    const date = localDateInput(cursor.getTime());
    if (costs.has(date)) {
      const cost = costs.get(date);
      // an unpriced day cannot be counted; say so instead of understating
      if (cost == null) incomplete = true;
      else spentUsd += cost;
    }
    series.push({ date, cumulativeUsd: spentUsd });
    cursor.setDate(cursor.getDate() + 1);
    cursor.setHours(0, 0, 0, 0);
  }

  const span = end.getTime() - start.getTime();
  const elapsed = Math.min(1, Math.max(0, (now - start.getTime()) / span));
  return {
    monthStart: start.getTime(),
    monthEnd: end.getTime(),
    spentUsd,
    incomplete,
    elapsed,
    percentUsed: (spentUsd / budgetUsd) * 100,
    paceUsd: elapsed > 0 ? spentUsd / elapsed : 0,
    series,
  };
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
    ['known_cost_usd', (r) => r.knownCostUsd == null ? '' : r.knownCostUsd.toFixed(4)],
    ['unpriced_requests', (r) => r.unpricedRequests ?? ''],
  ];
  return [columns.map(([name]) => name).join(','), ...rows.map((row) => columns.map(([, get]) => csvCell(get(row))).join(','))].join('\r\n') + '\r\n';
}

/** Session drill-down export: counters and identifiers, never any text. */
export function sessionsCsv(rows: readonly SessionRow[]): string {
  const columns: Array<[string, (row: SessionRow) => string | number]> = [
    ['session_id', (r) => r.sessionId],
    ['provider', (r) => r.provider],
    ['project', (r) => r.project],
    ['first_activity', (r) => r.firstTs],
    ['last_activity', (r) => r.lastTs],
    ['duration_ms', (r) => r.durationMs],
    ['models', (r) => r.models.join(' ')],
    ['input_tokens', (r) => r.inputTokens],
    ['cache_write_tokens', (r) => r.cacheWriteTokens],
    ['cache_read_tokens', (r) => r.cacheReadTokens],
    ['output_tokens', (r) => r.outputTokens],
    ['reasoning_tokens', (r) => r.reasoningTokens],
    ['total_tokens', (r) => r.totalTokens],
    ['requests', (r) => r.requests],
    ['estimated_cost_usd', (r) => r.estimatedCostUsd == null ? '' : r.estimatedCostUsd.toFixed(4)],
    ['known_cost_usd', (r) => r.knownCostUsd == null ? '' : r.knownCostUsd.toFixed(4)],
    ['unpriced_requests', (r) => r.unpricedRequests ?? ''],
  ];
  return [columns.map(([name]) => name).join(','), ...rows.map((row) => columns.map(([, get]) => csvCell(get(row))).join(','))].join('\r\n') + '\r\n';
}
