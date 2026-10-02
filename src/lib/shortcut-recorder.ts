// Turns a key press into the accelerator string the backend parses
// (`Ctrl+Alt+U`, see src-tauri/src/window/shortcuts.rs). [FRONTEND]
//
// Rune-free for `tests/shortcut-recorder.unit.ts`. Every key name produced here
// is parsed by a Rust test (`every_recorder_key_name_parses`), so the two sides
// cannot drift apart.

export interface KeyInput {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type RecorderResult =
  | { kind: 'cancel' }
  | { kind: 'clear' }
  /** only modifiers so far, or a key that cannot be bound: keep listening */
  | { kind: 'pending'; modifiers: string[]; reason?: 'no-modifier' | 'unsupported-key' }
  | { kind: 'accelerator'; value: string };

const NAMED: Record<string, string> = {
  Space: 'Space', Enter: 'Enter', NumpadEnter: 'Enter', Tab: 'Tab',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
  Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
  Insert: 'Insert', Delete: 'Delete',
};

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/;

/** Backend key name for a physical key, or null when it cannot be bound. */
export function keyName(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^(?:Digit|Numpad)([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return NAMED[code] ?? null;
}

export function modifiersOf(e: KeyInput): string[] {
  const out: string[] = [];
  if (e.ctrlKey) out.push('Ctrl');
  if (e.altKey) out.push('Alt');
  if (e.shiftKey) out.push('Shift');
  if (e.metaKey) out.push('Super');
  return out;
}

/**
 * Feed every keydown of a recording session through this.
 * Esc cancels, Backspace (alone) clears, a modifier alone keeps listening, and
 * a key without a modifier is refused (a bare `U` would be swallowed
 * desktop-wide, which the backend rejects too).
 */
export function recordKey(e: KeyInput): RecorderResult {
  const mods = modifiersOf(e);
  if (e.code === 'Escape' && mods.length === 0) return { kind: 'cancel' };
  if (e.code === 'Backspace' && mods.length === 0) return { kind: 'clear' };
  if (MODIFIER_CODES.test(e.code)) return { kind: 'pending', modifiers: mods };
  const name = keyName(e.code);
  if (name === null) return { kind: 'pending', modifiers: mods, reason: 'unsupported-key' };
  if (mods.length === 0) return { kind: 'pending', modifiers: mods, reason: 'no-modifier' };
  return { kind: 'accelerator', value: [...mods, name].join('+') };
}
