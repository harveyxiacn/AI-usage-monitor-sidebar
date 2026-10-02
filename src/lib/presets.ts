// Settings presets: built-in ones and the user's own. [FRONTEND]
//
// A preset is a partial settings patch applied in one write through the normal
// `settings.patch()` / `update_settings` merge, so the backend clamps and
// validates it like any other input. Rune-free for `tests/presets.unit.ts`.
import { diffSettings, type SettingChange } from './settings-diff';
import { mergeSettings, type SettingsPatch } from './settings-writer';
import type { Settings } from './types';

export type BuiltinPresetId = 'minimal' | 'power' | 'screenShare' | 'cyber';

export const MAX_CUSTOM_PRESETS = 10;
export const MAX_PRESET_NAME = 40;

export const BUILTIN_PRESETS: Record<BuiltinPresetId, SettingsPatch> = {
  // as little as possible: one ring per provider, no labels, hides when idle
  minimal: {
    ringMode: 'primary',
    surfaceStyle: 'solid',
    percentPosition: 'below',
    autoHide: true,
    sidebarItems: { fiveHour: true, weekly: true, scoped: false, other: false, logo: true, percentLabel: false, moreButton: false },
  },
  // every window of every provider, labels in the rings, quick refresh, alerts
  power: {
    ringMode: 'all',
    surfaceStyle: 'glass',
    percentPosition: 'center',
    refreshIntervalSec: 30,
    adaptiveRefresh: true,
    notifications: true,
    forecastNotifications: true,
    sidebarItems: { fiveHour: true, weekly: true, scoped: true, other: true, logo: true, percentLabel: true, moreButton: true },
  },
  // nothing identifying on screen, nothing popping up over a call
  screenShare: {
    hideAccountEmail: true,
    notifications: false,
    autoHide: true,
    autoHideDelayMs: 500,
    popoverTimeoutSec: 5,
  },
  cyber: {
    theme: 'dark',
    surfaceStyle: 'cyber',
    cyberAccent: 'neon',
    ringMode: 'concentric',
    percentPosition: 'center',
    opacity: 1,
  },
};

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
