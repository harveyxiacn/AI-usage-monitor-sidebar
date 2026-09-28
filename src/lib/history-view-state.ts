import type { Bucket, ProviderId } from './types';
import type { HistoryPreset } from './history';

export const historyViewState = {
  preset: '7d' as HistoryPreset, customFrom: '', customTo: '', bucket: 'day' as Bucket,
  provider: '' as ProviderId | '', groupByModel: true, groupByProject: false, project: null as string | null,
  metric: 'tokens' as 'tokens' | 'cost', chartLayout: 'grouped' as 'grouped' | 'stacked',
  tableView: 'buckets' as 'buckets' | 'sessions', heatView: 'calendar' as 'calendar' | 'punchcard',
};
