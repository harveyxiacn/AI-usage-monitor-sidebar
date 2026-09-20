import type { HistoryRow } from './types';

export type HistoryPreset = 'today' | '7d' | '30d' | '90d' | 'custom';
export interface HistoryRange { from: number; to: number }

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
