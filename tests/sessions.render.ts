import { test, expect } from '@playwright/test';

// Deliberately no pointer, keyboard, dispatchEvent, or element.click calls.
// These checks load real routes, observe rendered output, and capture screenshots.
for (const theme of ['dark', 'light']) {
  test(`sessions workspace renders ${theme} without browser errors`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    const settings = encodeURIComponent(JSON.stringify({ theme, language: 'en' }));
    await page.goto(`/dashboard?tab=sessions&provider=codex&session=demo-1&settings=${settings}`);
    await expect(page.getByRole('heading', { name: 'Sessions', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Improve startup loading', exact: true })).toBeVisible();
    await expect(page.getByText('Conversation content is off.', { exact: false })).toBeVisible();
    await expect(page.locator('.session-row')).toHaveCount(25);
    expect(errors).toEqual([]);
    await page.screenshot({ path: `test-results/sessions-${theme}.png`, fullPage: true });
  });
}
test('Chinese session metrics and assessment setup render at narrow width', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.setViewportSize({ width: 700, height: 850 });
  const settings = encodeURIComponent(JSON.stringify({ language: 'zh-CN' }));
  await page.goto(`/dashboard?tab=sessions&provider=codex&session=demo-1&pane=metrics&settings=${settings}`);
  await expect(page.getByRole('heading', { name: '会话', exact: true })).toBeVisible();
  await expect(page.getByText('估算活跃时间', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.goto(`/dashboard?tab=sessions&provider=codex&session=demo-1&pane=evaluation&settings=${settings}`);
  await expect(page.getByRole('button', { name: '准备脱敏预览', exact: true })).toBeDisabled();
  await expect(page.getByText('暂无评测报告，请先准备并检查预览。', { exact: true })).toBeVisible();
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/sessions-zh-narrow.png', fullPage: true });
});
test('history charts render with bounded table rows', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/dashboard?tab=history');
  await expect(page.locator('canvas').first()).toBeVisible();
  await expect(page.locator('.table-wrap tbody tr').first()).toBeVisible();
  expect(await page.locator('.table-wrap tbody tr').count()).toBeLessThanOrEqual(100);
  const chartId = await page.evaluate(async () => {
    const path = performance.getEntriesByType('resource').map(entry => entry.name).find(name => name.includes('/deps/chart__js.js'))!;
    const { Chart } = await import(path);
    const canvas = document.querySelector('.chart canvas') as HTMLCanvasElement;
    return Chart.getChart(canvas)?.id;
  });
  expect(chartId).toBeDefined();
  await page.evaluate(async () => {
    const path = '/src/lib/mock.ts';
    const { mockEmit } = await import(path);
    mockEmit('ingest-progress', { running: false, eventsAdded: 1, filesScanned: 1, filesUpdated: 1, durationMs: 1, errors: [] });
  });
  await page.waitForTimeout(300);
  // Data events are backend fixtures, never synthetic pointer or keyboard input.
  await expect.poll(() => page.evaluate(async () => {
    const path = performance.getEntriesByType('resource').map(entry => entry.name).find(name => name.includes('/deps/chart__js.js'))!;
    const { Chart } = await import(path);
    return Chart.getChart(document.querySelector('.chart canvas') as HTMLCanvasElement)?.id;
  })).toBe(chartId);
  expect(errors).toEqual([]);
});

test('saved AI and user-reviewed reports render evidence without sending live data', async ({ page }) => {
  await page.route('**/src/lib/session-mock.ts*', async route => {
    const response = await route.fetch();
    const source = await response.text();
    // Seed only the browser mock module in this page, never production code.
    const seed = `
await sessionMockInvoke('save_analysis_settings', {settings:{contentEnabled:true,endpoint:'https://example.invalid/v1/chat/completions',model:'synthetic-only',apiKeyEnv:'OPENAI_API_KEY',maxInputChars:60000,maxOutputTokens:3000,inputUsdPerMillion:null,outputUsdPerMillion:null}});
const fixturePreview = await sessionMockInvoke('prepare_session_evaluation', {provider:'codex',sessionId:'demo-1'});
const fixtureReport = await sessionMockInvoke('evaluate_session', {preview:fixturePreview});
fixtureReport.analysis.requirements.push({...fixtureReport.analysis.requirements[0],id:'r2',text:'Human-reviewed requirement',confirmedByUser:true});
await sessionMockInvoke('save_evaluation_review', {id:fixtureReport.id,requirements:fixtureReport.analysis.requirements});
`;
    await route.fulfill({ response, body: source + '\n' + seed });
  });
  await page.goto('/dashboard?tab=sessions&provider=codex&session=demo-1&pane=evaluation');
  await expect(page.getByText('AI proposal · unconfirmed', { exact: true })).toBeVisible();
  await expect(page.getByText('User reviewed', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Evidence: demo-1-m1', exact: true })).toHaveCount(2);
  await expect(page.getByLabel('Confirmed by me').first()).not.toBeChecked();
  await expect(page.getByLabel('Confirmed by me').nth(1)).toBeChecked();
  await page.locator('.report').screenshot({ path: 'test-results/sessions-report.png' });
});

test('long redacted preview renders within a narrow viewport', async ({ page }) => {
  await page.setViewportSize({ width: 650, height: 850 });
  await page.goto('/dashboard?tab=sessions');
  await expect(page.getByRole('heading', { name: 'Sessions', exact: true })).toBeVisible();
  await page.evaluate(async () => { const path = '/tests/preview-fixture.ts'; (await import(path)).mountPreview(); });
  await expect(page.getByLabel('Review exactly what will be sent')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Send this preview for assessment' })).toBeEnabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect((await page.locator('textarea').inputValue()).length).toBeGreaterThan(10000);
  await page.screenshot({ path: 'test-results/sessions-preview.png', fullPage: true });
});

// Insights deep link: no pointer or keyboard input, only observation.
for (const theme of ['dark', 'light']) {
  test(`session insights render ${theme} with charts, ranked lists and tool usage`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    const settings = encodeURIComponent(JSON.stringify({ theme, language: 'en' }));
    await page.goto(`/dashboard?tab=sessions&view=insights&provider=codex&settings=${settings}`);
    await expect(page.getByText('Median cost / session', { exact: true })).toBeVisible();
    await expect(page.getByText('Tool failure rate', { exact: true }).first()).toBeVisible();
    await expect(page.locator('canvas')).toHaveCount(3);
    await expect(page.getByRole('heading', { name: 'Most expensive sessions', exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Tool usage', exact: true })).toBeVisible();
    await expect(page.locator('.chip').first()).toBeVisible();
    await expect(page.locator('.session-row')).toHaveCount(0);
    expect(errors).toEqual([]);
    await page.screenshot({ path: `test-results/session-insights-${theme}.png`, fullPage: true });
  });
}
test('Chinese session insights render at narrow width without horizontal overflow', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.setViewportSize({ width: 700, height: 850 });
  const settings = encodeURIComponent(JSON.stringify({ language: 'zh-CN' }));
  await page.goto(`/dashboard?tab=sessions&view=insights&settings=${settings}`);
  await expect(page.getByText('会话费用中位数', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '工具使用', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  expect(errors).toEqual([]);
  await page.screenshot({ path: 'test-results/session-insights-zh.png', fullPage: true });
});
