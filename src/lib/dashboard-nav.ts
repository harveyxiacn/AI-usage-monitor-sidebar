// In-window tab switching for panels that live below the dashboard page.
// The page listens for this event; the Tauri `dashboard-navigate` event is
// for requests that come from other windows. [FRONTEND]
import type { DashboardTab } from './types';

export const DASHBOARD_TAB_EVENT = 'dashboard-tab-request';
export function requestDashboardTab(tab: DashboardTab) {
  if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent<DashboardTab>(DASHBOARD_TAB_EVENT, { detail: tab }));
}
