// Screen-sharing privacy helpers. [FRONTEND]
//
// Deliberately dependency-free (no i18n, no stores): masking must be a pure
// string function so it can be unit-tested in plain node and reused from any
// window, and so nothing about *when* to mask is baked into *how*.

/** The ellipsis stand-in for every hidden run of characters. */
const MASK = '•••';

/** First code point of `s`, so a CJK name or an emoji survives intact. */
function firstChar(s: string): string {
  return Array.from(s)[0] ?? '';
}

/**
 * `harvey@gmail.com` → `h•••@g•••.com`.
 *
 * Keeps the first character of the local part, the first character of the
 * domain and the last domain label: enough for the owner to recognise their
 * own account in a screenshot, useless to everyone else watching. Input that
 * is not an address (no `@`) collapses into a single run rather than being
 * echoed — an unrecognised shape must never leak in full.
 */
export function maskEmail(email: string | null | undefined): string {
  const value = (email ?? '').trim();
  if (value === '') return '';
  const at = value.lastIndexOf('@');
  if (at < 0) return firstChar(value) + MASK;
  const local = value.slice(0, at);
  const domain = value.slice(at + 1);
  const head = local === '' ? MASK : firstChar(local) + MASK;
  if (domain === '') return `${head}@`;
  const dot = domain.lastIndexOf('.');
  const tail =
    dot <= 0 ? firstChar(domain) + MASK : `${firstChar(domain)}${MASK}${domain.slice(dot)}`;
  return `${head}@${tail}`;
}

/**
 * An account e-mail as it may be rendered. Every place that puts an address on
 * screen — text, a `title`, an export — goes through this, so turning
 * `Settings.hideAccountEmail` on can never miss one of them.
 */
export function accountEmail(email: string | null | undefined, hide: boolean): string {
  const value = (email ?? '').trim();
  return hide ? maskEmail(value) : value;
}
