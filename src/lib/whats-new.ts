// "What's new": choosing and splitting the bundled release notes. [FRONTEND]
//
// The notes are `docs/releases/v<version>.md`, bundled by `release-notes.ts`.
// Each file has a `# title`, then a `## 中文` and a `## English` section.
import type { Language } from './types';

/** `1.2.3` / `v1.2.3-rc.1` → numeric parts; null when it is not a version. */
export function parseVersion(value: string): number[] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)/.exec(value.trim());
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

/** Negative when `a` is older than `b`; NaN when either is not a version. */
export function compareVersions(a: string, b: string): number {
  const x = parseVersion(a);
  const y = parseVersion(b);
  if (!x || !y) return Number.NaN;
  for (let i = 0; i < 3; i += 1) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}

/** The bundled file path for `version`, or null when there are no notes for it. */
export function releaseNotesKey(paths: string[], version: string): string | null {
  if (!parseVersion(version)) return null;
  return (
    paths.find((p) => {
      const file = /v(\d+\.\d+\.\d+)\.md$/.exec(p)?.[1];
      return file !== undefined && compareVersions(file, version) === 0;
    }) ?? null
  );
}

export interface ReleaseNotes {
  title: string;
  zh: string;
  en: string;
}

/** Split a release file into its title and the two language sections. */
export function splitReleaseNotes(markdown: string): ReleaseNotes {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  let title = '';
  const sections: Record<'zh' | 'en', string[]> = { zh: [], en: [] };
  let current: 'zh' | 'en' | null = null;
  for (const line of lines) {
    const h2 = /^##\s+(.*)$/.exec(line);
    if (h2) {
      const name = h2[1].trim().toLowerCase();
      current = name === '中文' || name === 'zh' || name === 'zh-cn' ? 'zh' : name === 'english' || name === 'en' ? 'en' : null;
      continue;
    }
    const h1 = /^#\s+(.*)$/.exec(line);
    if (h1 && !title) {
      title = h1[1].trim();
      continue;
    }
    if (current) sections[current].push(line);
  }
  return { title, zh: sections.zh.join('\n').trim(), en: sections.en.join('\n').trim() };
}

/** The section for the UI language, falling back to the other one. */
export function notesFor(notes: ReleaseNotes, locale: 'en' | 'zh-CN'): string {
  const [first, second] = locale === 'zh-CN' ? [notes.zh, notes.en] : [notes.en, notes.zh];
  return first || second;
}

export interface WhatsNewInput {
  onboarded: boolean;
  lastSeenVersion: string;
  currentVersion: string;
  hasNotes: boolean;
}

/**
 * Show the panel once after the version changed. A brand-new install is still
 * in the wizard (`onboarded` false) and never sees it; the wizard records the
 * running version when it ends.
 */
export function shouldShowWhatsNew(input: WhatsNewInput): boolean {
  return input.onboarded && input.hasNotes && !!input.currentVersion && input.lastSeenVersion !== input.currentVersion;
}

/** Language setting → the two catalogues the notes come in. */
export function localeOf(language: Language, navigatorLanguage: string): 'en' | 'zh-CN' {
  if (language === 'en' || language === 'zh-CN') return language;
  return navigatorLanguage.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}
