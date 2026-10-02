// Bundled release notes. [FRONTEND]
//
// `docs/releases/v*.md` are the very files published with each GitHub release;
// Vite inlines them at build time as lazy chunks, so the dashboard carries no
// network dependency and loads only the file it needs.
import { releaseNotesKey, splitReleaseNotes, type ReleaseNotes } from './whats-new';

const files = import.meta.glob('../../docs/releases/v*.md', { query: '?raw', import: 'default' }) as Record<
  string,
  () => Promise<string>
>;

/** True when the build contains notes for `version` (cheap, no loading). */
export function hasReleaseNotes(version: string): boolean {
  return releaseNotesKey(Object.keys(files), version) !== null;
}

export async function loadReleaseNotes(version: string): Promise<ReleaseNotes | null> {
  const key = releaseNotesKey(Object.keys(files), version);
  return key ? splitReleaseNotes(await files[key]()) : null;
}
