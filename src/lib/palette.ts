// Command palette: the command shape and the fuzzy matcher / ranker. [FRONTEND]
//
// Rune-free and DOM-free so `tests/palette.unit.ts` can exercise it. The
// commands themselves are built in `palette-commands.ts`.

export type PaletteGroup = 'navigate' | 'action' | 'setting';

/** Display / tie-break order of the groups. */
export const GROUP_ORDER: readonly PaletteGroup[] = ['navigate', 'action', 'setting'];

export interface PaletteCommand {
  /** Stable machine id (`nav:history:quota`, `action:refresh`, `toggle:autoHide`). */
  id: string;
  group: PaletteGroup;
  /** What the user reads and what matching favours. */
  title: string;
  /** Extra words that also match (other language, synonyms, the setting key). */
  keywords?: string;
  /** Short state shown at the right, e.g. "On" for a toggle. */
  hint?: string;
  run: () => void | Promise<void>;
}

/** Lower-cased, trimmed, whitespace collapsed. */
export function normalize(text: string): string {
  return text.toLocaleLowerCase().replace(/\s+/g, ' ').trim();
}

const WORD_START = /[\s\-_/.:·(]/;

/**
 * How well one query term matches a text; `null` = not at all.
 * Prefix of the text > start of a word > plain substring > the letters in
 * order with gaps (the "fuzzy" part; earlier and tighter is better).
 */
export function scoreTerm(term: string, text: string): number | null {
  if (term === '') return 0;
  if (text.startsWith(term)) return 100 - Math.min(20, text.length - term.length) * 0.5;
  const at = text.indexOf(term);
  if (at >= 0) {
    const boundary = at === 0 || WORD_START.test(text[at - 1]);
    return (boundary ? 80 : 60) - Math.min(20, at) * 0.5;
  }
  // subsequence: every letter of the term, in order
  let pos = -1;
  let first = -1;
  let gaps = 0;
  for (const ch of term) {
    const next = text.indexOf(ch, pos + 1);
    if (next < 0) return null;
    if (first < 0) first = next;
    else gaps += next - pos - 1;
    pos = next;
  }
  if (term.length < 3) return null; // two letters scattered over a title is noise
  return Math.max(5, 40 - gaps - first * 0.5);
}

/** Score of a whole query against a command, `null` when any term misses. */
export function scoreCommand(query: string, command: PaletteCommand): number | null {
  const terms = normalize(query).split(' ').filter(Boolean);
  if (terms.length === 0) return 0;
  const title = normalize(command.title);
  const extra = normalize(command.keywords ?? '');
  let total = 0;
  for (const term of terms) {
    const inTitle = scoreTerm(term, title);
    const inExtra = extra === '' ? null : scoreTerm(term, extra);
    // a hit in the title always beats the same hit in the extra words
    const best = Math.max(inTitle ?? -1, inExtra === null ? -1 : inExtra * 0.6);
    if (best < 0) return null;
    total += best;
  }
  return total;
}

/**
 * The commands matching `query`, best first. Ties keep the group order and
 * then the registry order, so the list is stable. An empty query returns
 * everything in registry order (grouped), capped at `limit`.
 */
export function rankCommands(query: string, commands: readonly PaletteCommand[], limit = 50): PaletteCommand[] {
  const scored: { command: PaletteCommand; score: number; index: number }[] = [];
  commands.forEach((command, index) => {
    const score = scoreCommand(query, command);
    if (score !== null) scored.push({ command, score, index });
  });
  const empty = normalize(query) === '';
  scored.sort(
    (a, b) =>
      (empty ? 0 : b.score - a.score) ||
      GROUP_ORDER.indexOf(a.command.group) - GROUP_ORDER.indexOf(b.command.group) ||
      a.index - b.index,
  );
  return scored.slice(0, limit).map((s) => s.command);
}
