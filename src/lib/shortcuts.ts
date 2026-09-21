// Client-side sanity check for the two global-shortcut settings. [FRONTEND]
//
// The authority is Rust (`src-tauri/src/window/shortcuts.rs`, which parses with
// global-hotkey and reports registration failures through `get_shortcut_status`).
// This mirror exists so the settings field can complain while the user types,
// and so the browser preview behaves like the desktop app. It deliberately
// accepts a little more than it understands: only shapes that are certainly
// wrong are rejected, the rest is left to the backend.

/** Modifier tokens global-hotkey accepts, upper-cased. */
const MODIFIERS = new Set([
  'ALT',
  'OPTION',
  'CONTROL',
  'CTRL',
  'COMMAND',
  'CMD',
  'SUPER',
  'SHIFT',
  'COMMANDORCONTROL',
  'COMMANDORCTRL',
  'CMDORCTRL',
  'CMDORCONTROL',
]);

/**
 * `null` when the value is usable (an empty value disables the shortcut),
 * otherwise a reason key for i18n: `empty-token`, `no-modifier`, `no-key` or
 * `too-many-keys`.
 */
export function shortcutProblem(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed === '') return null;

  const tokens = trimmed.split('+').map((token) => token.trim());
  if (tokens.some((token) => token === '')) return 'empty-token';

  const keys = tokens.filter((token) => !MODIFIERS.has(token.toUpperCase()));
  if (keys.length === 0) return 'no-key';
  if (keys.length > 1) return 'too-many-keys';
  // A bare key would be swallowed desktop-wide, so Rust refuses it too.
  if (tokens.length === keys.length) return 'no-modifier';
  return null;
}
