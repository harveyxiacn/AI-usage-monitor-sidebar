import type { Bucket, ProviderId } from './types';
import type { HeatMetric, HistoryPreset } from './history';

/** The History sub-views. */
export type HistoryView = 'usage' | 'quota' | 'cost' | 'commits';
export const HISTORY_VIEWS: readonly HistoryView[] = ['usage', 'quota', 'cost', 'commits'];

const VIEW_STORAGE_KEY = 'ai-usage-sidebar.history.view';

export const historyViewState = {
  view: 'usage' as HistoryView,
  preset: '7d' as HistoryPreset, customFrom: '', customTo: '', bucket: 'day' as Bucket,
  provider: '' as ProviderId | '', groupByModel: true, groupByProject: false, project: null as string | null,
  metric: 'tokens' as 'tokens' | 'cost', chartLayout: 'grouped' as 'grouped' | 'stacked',
  tableView: 'buckets' as 'buckets' | 'sessions', heatView: 'calendar' as 'calendar' | 'punchcard',
  heatMetric: 'tokens' as HeatMetric,
};

export const isHistoryView = (value: unknown): value is HistoryView =>
  typeof value === 'string' && (HISTORY_VIEWS as readonly string[]).includes(value);

let deepLinkConsumed = false;

/**
 * The sub-view to open with. A `?view=` deep link (`/dashboard?tab=history&view=quota`)
 * wins once per page load; afterwards the in-memory choice, then the one kept
 * in localStorage (a per-viewer convenience that may be unavailable).
 */
export function initialHistoryView(): HistoryView {
  if (typeof window === 'undefined') return historyViewState.view;
  if (!deepLinkConsumed) {
    deepLinkConsumed = true;
    try {
      const params = new URLSearchParams(window.location.search);
      const linked = params.get('view');
      // `&project=<exact cwd>` preselects the project filter (the Commits view needs one)
      const project = params.get('project');
      if (params.get('tab') === 'history' && project) historyViewState.project = project;
      if (params.get('tab') === 'history' && isHistoryView(linked)) {
        historyViewState.view = linked;
        return linked;
      }
      const stored = window.localStorage.getItem(VIEW_STORAGE_KEY);
      if (isHistoryView(stored)) historyViewState.view = stored;
    } catch {
      /* private window or blocked storage: keep the default */
    }
  }
  return historyViewState.view;
}

export function rememberHistoryView(view: HistoryView): void {
  historyViewState.view = view;
  try {
    window.localStorage.setItem(VIEW_STORAGE_KEY, view);
  } catch {
    /* not persisted; the in-memory choice still holds */
  }
}
