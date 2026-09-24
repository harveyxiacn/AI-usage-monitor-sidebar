// A failed first IPC read must not leave the sidebar on fallback settings for
// the rest of the process lifetime. This only navigates; no pointer input.
import { expect, test } from '@playwright/test';
import { mockSettings, mockSnapshot } from '../src/lib/mock';

test('sidebar restores the saved centre percentage after a transient startup read failure', async ({ page }) => {
  const runtimeErrors: string[] = [];
  page.on('pageerror', (error) => runtimeErrors.push(error.message));
  let reads = 0;
  const saved = { ...mockSettings, percentPosition: 'center' as const };
  await page.exposeBinding('__startupInvoke', (_source, command: string) => {
    switch (command) {
      case 'get_settings':
        if (++reads === 1) throw new Error('backend is still starting');
        return saved;
      case 'get_snapshot': return mockSnapshot;
      case 'sidebar_relayout': return;
      case 'plugin:event|listen': return 1;
      case 'plugin:event|unlisten': return;
      default: throw new Error(`Unexpected Tauri command: ${command}`);
    }
  });
  await page.addInitScript(() => {
    let callbackId = 0;
    const invoke = Reflect.get(window, '__startupInvoke') as (command: string, args: unknown) => Promise<unknown>;
    Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {
      transformCallback: () => ++callbackId,
      invoke: (command: string, args: unknown) => invoke(command, args),
    } });
    Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { value: { unregisterListener: () => {} } });
  });

  await page.goto('/');
  await expect(page.locator('.slot .center-pct')).toHaveCount(2, { timeout: 5_000 });
  await expect(page.locator('.slot .pct')).toHaveCount(0);
  expect(reads).toBeGreaterThanOrEqual(2);
  expect(runtimeErrors).toEqual([]);
});
