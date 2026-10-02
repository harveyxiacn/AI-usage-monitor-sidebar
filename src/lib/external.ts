// Small shared helpers for talking to the host system from the dashboard. [FRONTEND]
import { isTauri } from './api';

/** Open an http(s) URL in the system browser (a new tab in the browser preview). */
export async function openExternal(url: string): Promise<void> {
  if (isTauri()) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } else {
    window.open(url, '_blank', 'noopener');
  }
}

/** Copy text to the clipboard; WebKitGTK sometimes lacks the async API, so fall back to execCommand. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement('textarea');
    const focused = document.activeElement;
    area.value = text;
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand('copy');
    } catch {
      /* nothing else we can do */
    }
    area.remove();
    if (focused instanceof HTMLElement) focused.focus();
    return ok;
  }
}
