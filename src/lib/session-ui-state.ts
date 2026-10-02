import type { SessionListQuery } from './session-types';

/** In-memory navigation state only; transcript content never enters persistence. */
export const sessionViewState: { query: SessionListQuery; selected: { provider: string; sessionId: string } | null; view: 'browse' | 'insights' } = {
  query: { search: '', provider: null, project: null, sort: 'recent', offset: 0, limit: 25 }, selected: null, view: 'browse',
};
