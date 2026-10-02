// Settings presets: built-in ones and the user's own. [FRONTEND]
//
// A preset is a partial settings patch applied in one write through the normal
// `settings.patch()` / `update_settings` merge, so the backend clamps and
// validates it like any other input. Rune-free for `tests/presets.unit.ts`.
import builtinPresets from './builtin-presets.json';
import { diffSettings, type SettingChange } from './settings-diff';
import { mergeSettings, type SettingsPatch } from './settings-writer';
import type { Settings } from './types';

export type BuiltinPresetId = 'minimal' | 'power' | 'screenShare' | 'cyber';

export const MAX_CUSTOM_PRESETS = 10;
export const MAX_PRESET_NAME = 40;

/**
 * Shared with the Rust tray menu (`src-tauri/src/window/tray_presets.rs`
 * includes the same file), so the two lists cannot drift apart.
 */
export const BUILTIN_PRESETS = builtinPresets as Record<BuiltinPresetId, SettingsPatch>;

export const BUILTIN_IDS = Object.keys(BUILTIN_PRESETS) as BuiltinPresetId[];

/** What applying `patch` would change right now (empty = nothing to do). */
export function presetChanges(current: Settings, patch: SettingsPatch): SettingChange[] {
  return diffSettings(current, mergeSettings(current, patch));
}

/** Keys a custom preset captures: how the bar looks, not where it lives. */
const CAPTURED: readonly (keyof Settings)[] = [
  'theme', 'surfaceStyle', 'cyberAccent', 'scale', 'opacity', 'percentMode', 'percentPosition',
  'ringMode', 'sidebarItems', 'colors', 'sizes',
];

/** The current appearance and sidebar items as a preset patch. */
export function capturePreset(current: Settings): SettingsPatch {
  const patch: Record<string, unknown> = {};
  for (const key of CAPTURED) patch[key] = structuredClone(current[key]);
  return patch as SettingsPatch;
}

export type AddPresetResult =
  | { ok: true; presets: Settings['customPresets'] }
  | { ok: false; reason: 'name' | 'full' };

/** Add or replace a custom preset, enforcing the name rules and the cap. */
export function addCustomPreset(
  presets: Settings['customPresets'],
  name: string,
  patch: SettingsPatch,
): AddPresetResult {
  const trimmed = name.trim();
  if (trimmed === '' || Array.from(trimmed).length > MAX_PRESET_NAME) return { ok: false, reason: 'name' };
  if (!(trimmed in presets) && Object.keys(presets).length >= MAX_CUSTOM_PRESETS) return { ok: false, reason: 'full' };
  return { ok: true, presets: { ...presets, [trimmed]: patch as Record<string, unknown> } };
}

export function removeCustomPreset(presets: Settings['customPresets'], name: string): Settings['customPresets'] {
  const next = { ...presets };
  delete next[name];
  return next;
}
