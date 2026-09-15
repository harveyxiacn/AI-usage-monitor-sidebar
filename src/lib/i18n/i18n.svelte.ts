// Rune-based i18n store. [FRONTEND]
// `locale` is module-level $state, so every `t(...)` call inside a template
// re-runs when the language changes — no per-component subscription needed.
import type { Language } from '$lib/types';
import en from './en.json';
import zhCN from './zh-CN.json';

export type Locale = 'en' | 'zh-CN';
export type MessageKey = keyof typeof en;

const catalogs: Record<Locale, Record<string, string>> = {
  en,
  'zh-CN': zhCN,
};

let locale = $state<Locale>('en');

export function getLocale(): Locale {
  return locale;
}

/** 'auto' → zh-CN when the browser/OS language starts with 'zh', else en. */
export function resolveLocale(language: Language): Locale {
  if (language === 'en' || language === 'zh-CN') return language;
  const nav = typeof navigator !== 'undefined' ? navigator.language : 'en';
  return nav.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

/** Applies Settings.language; also keeps <html lang> in sync. */
export function setLanguage(language: Language): Locale {
  const next = resolveLocale(language);
  if (next !== locale) locale = next;
  if (typeof document !== 'undefined') document.documentElement.lang = next;
  return next;
}

export type TParams = Record<string, string | number>;

function interpolate(template: string, params?: TParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole
  );
}

/** Translate. Falls back to English, then to the key itself. */
export function t(key: MessageKey, params?: TParams): string {
  const raw = catalogs[locale][key] ?? catalogs.en[key] ?? key;
  return interpolate(raw, params);
}

/** True when the English catalogue has the key (used for optional hints). */
export function hasKey(key: string): boolean {
  return key in catalogs.en;
}

/**
 * Same as `t()` but for keys that are assembled at runtime
 * (`status.hint.claude.token_expired`, `settings.theme.dark`, …), where the
 * compiler cannot check the literal. Unknown keys fall back to the key itself.
 */
export function tDyn(key: string, params?: TParams): string {
  return t(key as MessageKey, params);
}

/** Intl locale tag used for date/number formatting. */
export function intlLocale(): string {
  return locale === 'zh-CN' ? 'zh-CN' : 'en-US';
}
