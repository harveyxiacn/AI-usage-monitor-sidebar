// Shared Settings store. [FRONTEND]
// One instance per window (each Tauri window is its own page load); the
// `settings-updated` event keeps the three windows in sync.
import { applyWindowSettings, getSettings, onSettingsUpdated, updateSettings, type Unlisten } from '$lib/api';
import type { ColorSettings, Settings, SizeSettings } from '$lib/types';

/** Contract defaults for the user-tunable palette (empty = keep the theme's). */
export const defaultColors: ColorSettings = {
  claude: '#ff5c1a',
  codex: '#10a37f',
  warn: '#f5c542',
  critical: '#ff3b30',
  surface: '',
  text: '',
};

/** Contract defaults for the user-tunable geometry, in CSS px at scale 1. */
export const defaultSizes: SizeSettings = {
  ringSize: 56,
  ringStroke: 4.5,
  barGap: 18,
  barPadding: 10,
  cornerRadius: 26,
  labelSize: 13,
};

/** [min, max, step] per size key — the UI and applyTheme both clamp with these. */
export const SIZE_LIMITS: Record<keyof SizeSettings, [min: number, max: number, step: number]> = {
  ringSize: [40, 96, 1],
  ringStroke: [3, 8, 0.5],
  barGap: [6, 40, 1],
  barPadding: [4, 24, 1],
  cornerRadius: [8, 40, 1],
  labelSize: [9, 18, 1],
};

export function clampSize<K extends keyof SizeSettings>(key: K, value: number): number {
  const [min, max] = SIZE_LIMITS[key];
  if (!Number.isFinite(value)) return defaultSizes[key];
  return Math.min(max, Math.max(min, value));
}

/** Every size key clamped into range, falling back to the default when absent. */
export function clampSizes(sizes: Partial<SizeSettings> | undefined): SizeSettings {
  const out = {} as SizeSettings;
  for (const key of Object.keys(defaultSizes) as (keyof SizeSettings)[]) {
    out[key] = clampSize(key, sizes?.[key] ?? defaultSizes[key]);
  }
  return out;
}

/** Frontend-side fallback so the first paint never has to null-check. */
export const defaultSettings: Settings = {
  version: 1,
  language: 'auto',
  theme: 'dark',
  surfaceStyle: 'glass',
  edge: 'right',
  verticalAlign: 'center',
  verticalOffset: 0,
  monitor: null,
  autoHide: false,
  autoHideDelayMs: 800,
  collapsedWidth: 6,
  ringMode: 'concentric',
  showScopedRing: true,
  percentMode: 'used',
  showPercentLabel: true,
  refreshIntervalSec: 60,
  providers: { claude: { enabled: true, order: 0 }, codex: { enabled: true, order: 1 } },
  ingestEnabled: true,
  autostart: false,
  opacity: 1,
  scale: 1,
  thresholds: { warn: 70, critical: 90 },
  colors: structuredClone(defaultColors),
  sizes: structuredClone(defaultSizes),
  notifications: false,
  alwaysOnTop: true,
};

/** Keys that require the platform layer to move/resize/restyle the windows. */
export const WINDOW_KEYS = ['surfaceStyle', 
  'edge',
  'verticalAlign',
  'verticalOffset',
  'monitor',
  'autoHide',
  'autoHideDelayMs',
  'collapsedWidth',
  'alwaysOnTop',
  'scale',
  'autostart',
] as const satisfies readonly (keyof Settings)[];

class SettingsStore {
  value = $state<Settings>(structuredClone(defaultSettings));
  loaded = $state(false);
  error = $state<string | null>(null);

  #refs = 0;
  #unlisten: Unlisten | null = null;

  /** Idempotent; returns a disposer to call from onDestroy/onMount cleanup. */
  init(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) void this.#start();
    return () => {
      this.#refs -= 1;
      if (this.#refs === 0) {
        this.#unlisten?.();
        this.#unlisten = null;
      }
    };
  }

  async #start() {
    try {
      this.value = await getSettings();
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loaded = true;
    }
    const un = await onSettingsUpdated((s) => {
      this.value = s;
    });
    // init() may already have been disposed while we awaited.
    if (this.#refs === 0) un();
    else this.#unlisten = un;
  }

  /**
   * Immediate-apply patch: optimistic local update, then persist. When a
   * window-related key changed the platform layer is asked to re-anchor.
   */
  async patch(patch: Partial<Settings>): Promise<void> {
    const previous = this.value;
    this.value = { ...previous, ...patch };
    const needsWindowUpdate = WINDOW_KEYS.some((k) => k in patch);
    try {
      this.value = await updateSettings(patch);
      this.error = null;
    } catch (e) {
      this.value = previous;
      this.error = String(e);
      return;
    }
    if (needsWindowUpdate) {
      try {
        await applyWindowSettings();
      } catch (e) {
        this.error = String(e);
      }
    }
  }

  /** Convenience for the per-provider record (shallow-merged by the backend). */
  async patchProvider(id: string, next: { enabled?: boolean; order?: number }): Promise<void> {
    const current = this.value.providers[id] ?? { enabled: true, order: 0 };
    await this.patch({ providers: { ...this.value.providers, [id]: { ...current, ...next } } });
  }
}

export const settings = new SettingsStore();
