import { test, expect } from '@playwright/test';
import { focusStatus, moveThreshold, thresholdZones, webhookHost, webhookUrlProblem } from '../src/lib/notifications';
import { downloadPercent, formatBytes, updateBannerVisible } from '../src/lib/update-banner';
import { mergeSettings } from '../src/lib/settings-writer';
import { mockSettings } from '../src/lib/mock';

test('moving one threshold never lets warn reach critical', () => {
  const start = { warn: 70, critical: 90 };
  expect(moveThreshold(start, 'warn', 80)).toEqual({ warn: 80, critical: 90 });
  expect(moveThreshold(start, 'warn', 95)).toEqual({ warn: 95, critical: 96 });
  expect(moveThreshold(start, 'critical', 50)).toEqual({ warn: 49, critical: 50 });
  expect(moveThreshold(start, 'critical', 70)).toEqual({ warn: 69, critical: 70 });
});

test('thresholds stay inside 1..100 and survive junk input', () => {
  expect(moveThreshold({ warn: 70, critical: 90 }, 'warn', -20)).toEqual({ warn: 1, critical: 90 });
  expect(moveThreshold({ warn: 70, critical: 90 }, 'critical', 400)).toEqual({ warn: 70, critical: 100 });
  expect(moveThreshold({ warn: 99, critical: 100 }, 'warn', 100)).toEqual({ warn: 99, critical: 100 });
  expect(moveThreshold({ warn: 70, critical: 90 }, 'warn', Number.NaN)).toEqual({ warn: 70, critical: 90 });
  expect(moveThreshold({ warn: 1, critical: 2 }, 'critical', 1)).toEqual({ warn: 1, critical: 2 });
});

test('the colour preview zones add up to the whole track', () => {
  expect(thresholdZones({ warn: 70, critical: 90 })).toEqual({ ok: 70, warn: 20, critical: 10 });
  const z = thresholdZones({ warn: 30, critical: 60 });
  expect(z.ok + z.warn + z.critical).toBe(100);
});

test('only https webhook URLs with a host are acceptable', () => {
  expect(webhookUrlProblem('https://ntfy.sh/my-topic')).toBeNull();
  expect(webhookUrlProblem('  https://hooks.slack.com/services/T/B/x ')).toBeNull();
  expect(webhookUrlProblem('')).toBe('empty');
  expect(webhookUrlProblem('   ')).toBe('empty');
  expect(webhookUrlProblem('ntfy.sh/topic')).toBe('not-a-url');
  expect(webhookUrlProblem('http://ntfy.sh/topic')).toBe('not-https');
  expect(webhookUrlProblem('file:///etc/passwd')).toBe('not-https');
});

test('only the host of a webhook URL is ever displayed', () => {
  expect(webhookHost('https://hooks.slack.com/services/T000/B000/SECRET')).toBe('hooks.slack.com');
  expect(webhookHost('nonsense')).toBe('');
});

test('focus status mirrors the backend rule', () => {
  expect(focusStatus(0, 1000)).toEqual({ kind: 'off' });
  expect(focusStatus(-1, 1000)).toEqual({ kind: 'forever' });
  expect(focusStatus(2000, 1000)).toEqual({ kind: 'until', until: 2000 });
  expect(focusStatus(1000, 1000)).toEqual({ kind: 'off' });
});

test('the webhook setting merges per key like the backend does', () => {
  const base = { ...mockSettings, webhook: { enabled: false, url: 'https://ntfy.sh/a', kind: 'ntfy' as const } };
  const next = mergeSettings(base, { webhook: { enabled: true } });
  expect(next.webhook).toEqual({ enabled: true, url: 'https://ntfy.sh/a', kind: 'ntfy' });
  expect(mockSettings.webhook).toEqual({ enabled: false, url: '', kind: 'generic' });
  expect(mockSettings.thresholdNotifications && mockSettings.budgetNotifications).toBe(true);
  expect(mockSettings.weeklySummary).toBe(false);

});

test('a skipped version silences the banner, a newer one brings it back', () => {
  expect(updateBannerVisible('0.6.0', '', false)).toBe(true);
  expect(updateBannerVisible('0.6.0', '0.6.0', false)).toBe(false);
  expect(updateBannerVisible('0.6.1', '0.6.0', false)).toBe(true);
  expect(updateBannerVisible('0.6.0', '', true)).toBe(false);
  expect(updateBannerVisible(null, '', false)).toBe(false);
});

test('download progress is a clamped percentage, or unknown without a total', () => {
  expect(downloadPercent(0, 1000)).toBe(0);
  expect(downloadPercent(500, 1000)).toBe(50);
  expect(downloadPercent(1500, 1000)).toBe(100);
  expect(downloadPercent(10, null)).toBeNull();
  expect(downloadPercent(10, 0)).toBeNull();
  expect(formatBytes(512)).toBe('512 B');
  expect(formatBytes(2048)).toBe('2 KB');
  expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
});
