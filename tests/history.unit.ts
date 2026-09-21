import { test, expect } from '@playwright/test';
import { csvCell, historyCsv, historyRange, historySeries, localDateInput, projectLabels, projectName } from '../src/lib/history';
import type { HistoryRow, ProviderId } from '../src/lib/types';

/** A history row carrying only the fields the chart/CSV helpers look at. */
function historyRow(
  bucketStart: string,
  provider: ProviderId,
  model: string | null,
  project: string | null,
  totalTokens: number,
  estimatedCostUsd: number | null = 0
): HistoryRow {
  return {
    bucketStart, provider, model, project,
    inputTokens: totalTokens, cacheWriteTokens: 0, cacheReadTokens: 0, outputTokens: 0,
    reasoningTokens: 0, totalTokens, requests: 1, estimatedCostUsd,
  };
}

test('custom ranges include the entire final local day and reject invalid dates', () => {
  const range = historyRange('custom', '2026-09-19', '2026-09-20')!;
  expect(localDateInput(range.from)).toBe('2026-09-19');
  expect(localDateInput(range.to - 1)).toBe('2026-09-20');
  expect(new Date(range.to).getHours()).toBe(0);
  expect(localDateInput(range.to)).toBe('2026-09-21');
  for (const [from, to] of [['', '2026-09-20'], ['2026-02-30', '2026-03-02'], ['2026-09-21', '2026-09-20']]) {
    expect(historyRange('custom', from, to)).toBeNull();
  }
});

test('presets contain seven calendar days including today', () => {
  const now = new Date(2026, 8, 20, 15, 30).getTime();
  const range = historyRange('7d', '', '', now)!;
  expect(localDateInput(range.from)).toBe('2026-09-14');
  expect(new Date(range.from).getHours()).toBe(0);
  expect(range.to).toBe(now);
});

test('calendar ranges survive daylight saving transitions', () => {
  const original = process.env.TZ;
  process.env.TZ = 'America/New_York';
  try {
    const spring = historyRange('custom', '2026-03-08', '2026-03-08')!;
    const fall = historyRange('custom', '2026-11-01', '2026-11-01')!;
    expect(spring.to - spring.from).toBe(23 * 3_600_000);
    expect(fall.to - fall.from).toBe(25 * 3_600_000);
  } finally {
    if (original === undefined) delete process.env.TZ;
    else process.env.TZ = original;
  }
});

test('project names handle Windows paths, roots, missing paths and duplicate basenames', () => {
  expect(projectName('/home/dev/workspace/api/')).toBe('api');
  expect(projectName('C:\\Users\\Dev\\workspace\\api\\')).toBe('api');
  expect(projectName('')).toBe('Unassigned');
  // only "" is unassigned (contract §4): a blank-looking path stays its own
  // project, quoted so the option is not rendered as an empty line
  expect(projectName('   ', '未归属')).toBe('"   "');
  expect(projectName('/')).toBe('/');
  expect(projectName('C:\\')).toBe('C:');
  const paths = ['/home/dev/team-a/api', '/home/dev/team-b/api', 'C:\\Projects\\dashboard', ''];
  const labels = projectLabels(paths, '未归属');
  expect(labels.get(paths[0])).toBe('api — /home/dev/team-a');
  expect(labels.get(paths[1])).toBe('api — /home/dev/team-b');
  expect(labels.get(paths[2])).toBe('dashboard');
  expect(labels.get('')).toBe('未归属');
});

test('historySeries keys series by provider, model and exact project path', () => {
  const rows = [
    historyRow('2026-09-20T00:00:00+08:00', 'codex', 'gpt-5', '/home/dev/team-a/api', 5),
    historyRow('2026-09-19T00:00:00+08:00', 'claude', 'opus', '/home/dev/team-b/api', 3),
    historyRow('2026-09-19T00:00:00+08:00', 'claude', 'opus', '/home/dev/team-a/api', 1),
    historyRow('2026-09-20T00:00:00+08:00', 'claude', 'opus', '/home/dev/team-a/api', 2),
    historyRow('2026-09-20T00:00:00+08:00', 'claude', 'opus', '', 7),
  ];
  const { buckets, series } = historySeries(rows, true, true, 'tokens');
  // buckets are chronological whatever order the rows arrive in
  expect(buckets).toEqual(['2026-09-19T00:00:00+08:00', '2026-09-20T00:00:00+08:00']);
  expect(series.map((s) => [s.provider, s.model, s.project])).toEqual([
    ['claude', 'opus', ''],
    ['claude', 'opus', '/home/dev/team-a/api'],
    ['claude', 'opus', '/home/dev/team-b/api'],
    ['codex', 'gpt-5', '/home/dev/team-a/api'],
  ]);
  // same basename, different path: totals must not merge
  expect(series.map((s) => s.values)).toEqual([[0, 7], [1, 2], [3, 0], [0, 5]]);
  expect(new Set(series.map((s) => s.key)).size).toBe(series.length);
});

test('historySeries collapses models and projects when grouping is off', () => {
  const rows = [
    historyRow('2026-09-19T00:00:00+08:00', 'claude', 'opus', '/a', 1),
    historyRow('2026-09-19T00:00:00+08:00', 'claude', 'sonnet', '/b', 2),
    historyRow('2026-09-19T00:00:00+08:00', 'codex', 'gpt-5', '/a', 4),
  ];
  const { series } = historySeries(rows, false, false, 'tokens');
  expect(series.map((s) => [s.provider, s.model, s.project, s.values])).toEqual([
    ['claude', null, null, [3]],
    ['codex', null, null, [4]],
  ]);
  // grouping by project alone keeps one series per provider and path
  const byProject = historySeries(rows, false, true, 'tokens').series;
  expect(byProject.map((s) => [s.provider, s.project, s.values])).toEqual([
    ['claude', '/a', [1]],
    ['claude', '/b', [2]],
    ['codex', '/a', [4]],
  ]);
});

test('historySeries reports an unknown cost bucket instead of understating it', () => {
  const rows = [
    historyRow('2026-09-19T00:00:00+08:00', 'codex', null, null, 10, 1.5),
    historyRow('2026-09-19T00:00:00+08:00', 'codex', null, null, 10, null),
    historyRow('2026-09-20T00:00:00+08:00', 'codex', null, null, 10, 2),
  ];
  expect(historySeries(rows, false, false, 'cost').series[0].values).toEqual([null, 2]);
  // tokens are always known, so the same rows still add up
  expect(historySeries(rows, false, false, 'tokens').series[0].values).toEqual([20, 10]);
});

test('CSV preserves delimiters, unknown cost and neutralizes formula cells', () => {
  expect(csvCell('hello,"world"\r\nnext')).toBe('"hello,""world""\r\nnext"');
  for (const value of ['=1+1', '+cmd', '-cmd', '@SUM(A1)', '\t=1+1']) {
    expect(csvCell(value)).toBe(`'${value}`);
  }
  expect(csvCell(-42)).toBe('-42');
  const project = 'C:\\团队\\comma, quote" project';
  const row: HistoryRow = {
    bucketStart: '2026-09-20T00:00:00Z', provider: 'claude', model: '模型,example', project,
    inputTokens: 100, cacheWriteTokens: 20, cacheReadTokens: 30, outputTokens: 40,
    reasoningTokens: 10, totalTokens: 190, requests: 1, estimatedCostUsd: null,
  };
  const csv = historyCsv([row]);
  expect(csv.split('\r\n')[0]).toBe(
    'bucket_start,provider,model,project,input_tokens,cache_write_tokens,cache_read_tokens,output_tokens,reasoning_tokens,total_tokens,requests,estimated_cost_usd'
  );
  expect(csv).toContain('"模型,example","C:\\团队\\comma, quote"" project",100,20,30,40,10,190,1,\r\n');
  // the export carries the complete path, never the shortened UI label
  expect(csv).toContain(project.replace(/"/g, '""'));
  // an unfiltered, ungrouped aggregate has no project: the column stays empty
  expect(historyCsv([{ ...row, project: null }])).toContain(',claude,"模型,example",,100,');
  expect(csv.split('\r\n')).toHaveLength(3);
});
