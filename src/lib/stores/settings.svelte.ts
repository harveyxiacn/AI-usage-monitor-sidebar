// Shared Settings store. [FRONTEND]
// One instance per window (each Tauri window is its own page load); the
// `settings-updated` event keeps the three windows in sync.
import { applyWindowSettings, getSettings, onSettingsUpdated, updateSettings, type Unlisten } from '$lib/api';
import { defaultColors, defaultSettings, defaultSizes } from '$lib/settings-defaults';
import { defaultSidebarItems } from '$lib/sidebar-items';
import type { Settings, SizeSettings } from '$lib/types';
import { SettingsWriter, type SettingsPatch } from '$lib/settings-writer';

export { defaultColors, defaultSettings, defaultSidebarItems, defaultSizes };

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
  saving = $state(false);

  #refs = 0;
  #unlisten: Unlisten | null = null;
  #generation = 0;
  #revision = 0;
  #reading: number | null = null;
  #retryTimer: ReturnType<typeof setTimeout> | null = null;
  #stopSync: (() => void) | null = null;
  #writer = new SettingsWriter(structuredClone(defaultSettings), updateSettings,
    (value, saving) => { this.value = value; this.saving = saving; },
    (error) => { this.error = String(error); },
    async (patch) => {
      if (WINDOW_KEYS.some((key) => key in patch)) await applyWindowSettings();
    });

  /** Idempotent; returns a disposer to call from onDestroy/onMount cleanup. */
  init(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) void this.#start();
    return () => {
      this.#refs -= 1;
      if (this.#refs === 0) {
        this.#generation++;
        this.#stopSync?.();
        this.#stopSync = null;
        clearTimeout(this.#retryTimer ?? undefined);
        this.#retryTimer = null;
        this.#unlisten?.();
        this.#unlisten = null;
      }
    };
  }

  #start() {
    const generation = ++this.#generation;
    // The sidebar can start before its event subscription is ready. Read the
    // backend in parallel, then reconcile again after a transient IPC failure
    // or when a long-lived window becomes visible.
    const sync = () => { if (!document.hidden) void this.#read(generation); };
    const resume = () => { if (!document.hidden) sync(); };
    const timer = setInterval(sync, 30_000);
    window.addEventListener('focus', resume);
    window.addEventListener('pageshow', resume);
    document.addEventListener('visibilitychange', resume);
    this.#stopSync = () => {
      clearInterval(timer);
      window.removeEventListener('focus', resume);
      window.removeEventListener('pageshow', resume);
      document.removeEventListener('visibilitychange', resume);
    };
    void onSettingsUpdated((value) => {
      if (generation !== this.#generation) return;
      this.#revision++;
      this.#writer.receive(value);
      this.loaded = true;
      this.error = null;
      clearTimeout(this.#retryTimer ?? undefined);
      this.#retryTimer = null;
    }).then((un) => {
      if (generation !== this.#generation) un();
      else this.#unlisten = un;
    }).catch((e) => {
      if (generation === this.#generation && !this.loaded) this.error = String(e);
    });
    sync();
  }

  async #read(generation: number) {
    if (generation !== this.#generation || this.#reading === generation) return;
    this.#reading = generation;
    const revision = this.#revision;
    try {
      const value = await getSettings();
      if (generation !== this.#generation) return;
      if (revision === this.#revision) this.#writer.receive(value);
      this.loaded = true;
      this.error = null;
      clearTimeout(this.#retryTimer ?? undefined);
      this.#retryTimer = null;
    } catch (e) {
      if (generation === this.#generation && !this.loaded) {
        this.error = String(e);
        if (this.#retryTimer === null) {
          this.#retryTimer = setTimeout(() => {
            this.#retryTimer = null;
            void this.#read(generation);
          }, 1_000);
        }
      }
    } finally {
      if (this.#reading === generation) this.#reading = null;
    }
  }

  /**
   * Immediate-apply patch: optimistic local update, then persist. When a
   * window-related key changed the platform layer is asked to re-anchor.
   */
  patch(patch: SettingsPatch): Promise<void> {
    this.error = null;
    return this.#writer.patch(patch);
  }

  /** Convenience for the per-provider record (shallow-merged by the backend). */
  async patchProvider(
    id: string,
    next: { enabled?: boolean; showInSidebar?: boolean; order?: number }
  ): Promise<void> {
    await this.patch({ providers: { [id]: next } });
  }
}

export const settings = new SettingsStore();
