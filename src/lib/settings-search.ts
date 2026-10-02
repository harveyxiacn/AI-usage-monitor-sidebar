// Settings-tab search: does a control's label/hint match what was typed? [FRONTEND]

/** Lower-cased, trimmed, whitespace-collapsed. */
export function normalizeQuery(query: string): string {
  return query.toLocaleLowerCase().replace(/\s+/g, ' ').trim();
}

/**
 * Should a control show? `matches` is its search result, `advanced` its tier.
 * Outside the Settings tab (`search` undefined) everything shows.
 */
export function controlShown(search: { revealsAdvanced: boolean } | undefined, advanced: boolean, matches: boolean): boolean {
  return matches && (!advanced || !search || search.revealsAdvanced);
}

/**
 * Every whitespace-separated term of `query` must occur in the joined `texts`
 * (case-insensitive), so "ring percent" finds a control whose label has one
 * and hint the other. An empty query matches everything.
 */
export function matchesQuery(query: string, ...texts: (string | undefined | null)[]): boolean {
  const q = normalizeQuery(query);
  if (q === '') return true;
  const haystack = normalizeQuery(texts.filter(Boolean).join(' '));
  return q.split(' ').every((term) => haystack.includes(term));
}
