// Navigation requests from the command palette to the dashboard's panels. [FRONTEND]
//
// The tab itself switches through `requestDashboardTab`. A sub-view is stored
// first (so a tab that mounts afterwards opens on it) and announced by an
// event (so a tab that is already mounted switches too). A Settings card is
// scrolled to once the lazily loaded tab has rendered it.
import { requestDashboardTab } from './dashboard-nav';
import { rememberHistoryView, type HistoryView } from './history-view-state';
import { sessionViewState } from './session-ui-state';

export const VIEW_REQUEST_EVENT = 'dashboard-view-request';

export type ViewRequest =
  | { tab: 'history'; view: HistoryView }
  | { tab: 'sessions'; view: 'browse' | 'insights' };

function announce(detail: ViewRequest): void {
  if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent<ViewRequest>(VIEW_REQUEST_EVENT, { detail }));
}

export function requestHistoryView(view: HistoryView): void {
  rememberHistoryView(view);
  requestDashboardTab('history');
  announce({ tab: 'history', view });
}

export function requestSessionsView(view: 'browse' | 'insights'): void {
  sessionViewState.view = view;
  requestDashboardTab('sessions');
  announce({ tab: 'sessions', view });
}

/**
 * Scroll the element with `id` to the top, waiting up to ~3 s for it to exist.
 * Cards above it keep loading for a moment (the price list, the changelog),
 * which pushes it down again, so the position is re-asserted for about a
 * second, and dropped at the first thing the user does.
 */
export function scrollToWhenPresent(id: string, tries = 60): void {
  const el = document.getElementById(id);
  if (!el) {
    if (tries > 0) setTimeout(() => scrollToWhenPresent(id, tries - 1), 50);
    return;
  }
  let cancelled = false;
  const cancel = () => (cancelled = true);
  const events = ['wheel', 'touchstart', 'pointerdown', 'keydown'] as const;
  for (const name of events) window.addEventListener(name, cancel, { once: true, passive: true });
  const settle = (delays: number[]) => {
    if (cancelled || !el.isConnected) return;
    el.scrollIntoView({ behavior: 'auto', block: 'start' });
    const [next, ...rest] = delays;
    if (next === undefined) {
      for (const name of events) window.removeEventListener(name, cancel);
      return;
    }
    setTimeout(() => settle(rest), next);
  };
  settle([100, 250, 500, 800]);
}

export function requestSettingsCard(id: string): void {
  requestDashboardTab('settings');
  scrollToWhenPresent(id);
}
