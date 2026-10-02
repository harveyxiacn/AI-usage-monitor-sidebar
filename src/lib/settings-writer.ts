import type { Settings } from './types';

/**
 * Spellings an old settings file or a saved preset may still use. The backend
 * reads them (src-tauri/src/settings.rs `migrate_legacy_keys`) and never
 * writes them; `normalizePatch` is the same rule for previews.
 */
export interface LegacySettingsKeys {
  /** @deprecated now `sidebarItems.scoped` */
  showScopedRing?: boolean;
  /** @deprecated now `sidebarItems.percentLabel` */
  showPercentLabel?: boolean;
}

/** Nested fields are merged by the backend; use the same rule for previews. */
export type SettingsPatch = LegacySettingsKeys & Partial<
  Omit<Settings, 'colors' | 'sizes' | 'thresholds' | 'providers' | 'sidebarItems' | 'subscriptionUsd' | 'webhook'>
> & {
  colors?: Partial<Settings['colors']>;
  sizes?: Partial<Settings['sizes']>;
  thresholds?: Partial<Settings['thresholds']>;
  providers?: Record<string, Partial<Settings['providers'][string]>>;
  sidebarItems?: Partial<Settings['sidebarItems']>;
  subscriptionUsd?: Record<string, number>;
  webhook?: Partial<Settings['webhook']>;
};

/**
 * Move the deprecated top-level `showScopedRing` / `showPercentLabel` into
 * `sidebarItems`; when a patch carries both spellings the nested one wins.
 * Returns `patch` itself when there is nothing to move.
 */
export function normalizePatch(patch: SettingsPatch): SettingsPatch {
  const { showScopedRing, showPercentLabel, ...rest } = patch;
  if (typeof showScopedRing !== 'boolean' && typeof showPercentLabel !== 'boolean') {
    return 'showScopedRing' in patch || 'showPercentLabel' in patch ? rest : patch;
  }
  const legacy: Partial<Settings['sidebarItems']> = {};
  if (typeof showScopedRing === 'boolean') legacy.scoped = showScopedRing;
  if (typeof showPercentLabel === 'boolean') legacy.percentLabel = showPercentLabel;
  return { ...rest, sidebarItems: { ...legacy, ...rest.sidebarItems } };
}

export function mergeSettings(base: Settings, rawPatch: SettingsPatch): Settings {
  const patch = normalizePatch(rawPatch);
  const providers = { ...base.providers };
  for (const [id, next] of Object.entries(patch.providers ?? {})) {
    providers[id] = { ...(providers[id] ?? { enabled: true, showInSidebar: true, order: 0 }), ...next };
  }
  return {
    ...base, ...patch, providers,
    colors: { ...base.colors, ...patch.colors },
    sizes: { ...base.sizes, ...patch.sizes },
    thresholds: { ...base.thresholds, ...patch.thresholds },
    sidebarItems: { ...base.sidebarItems, ...patch.sidebarItems },
    subscriptionUsd: { ...base.subscriptionUsd, ...patch.subscriptionUsd },
    webhook: { ...base.webhook, ...patch.webhook },
  };

}

/** Serial persistence with optimistic overlays: an old save/rollback never erases a later edit. */
export class SettingsWriter {
  #base: Settings;
  #revision = 0;
  #queue: Array<{ patch: SettingsPatch; resolve: () => void }> = [];
  #running = false;

  constructor(initial: Settings, private persist: (patch: SettingsPatch) => Promise<Settings>,
    private changed: (value: Settings, pending: boolean) => void,
    private failed: (error: unknown) => void,
    private saved: (patch: SettingsPatch) => Promise<void> = async () => {}) {
    this.#base = initial;
  }

  receive(value: Settings): void {
    this.#base = value;
    this.#revision++;
    this.#publish();
  }

  patch(patch: SettingsPatch): Promise<void> {
    return new Promise((resolve) => {
      this.#queue.push({ patch, resolve });
      this.#publish();
      void this.#drain();
    });
  }

  #publish(): void {
    this.changed(this.#queue.reduce((value, item) => mergeSettings(value, item.patch), this.#base), this.#queue.length > 0);
  }

  async #drain(): Promise<void> {
    if (this.#running) return;
    this.#running = true;
    while (this.#queue.length > 0) {
      const item = this.#queue[0];
      const revision = this.#revision;
      let committed = false;
      try {
        const value = await this.persist(item.patch);
        // A settings event received during invoke is at least as current as
        // its response. Preserve external-window changes in that case.
        if (revision === this.#revision) this.#base = value;
        committed = true;
      } catch (error) {
        this.failed(error);
      }
      this.#queue.shift();
      this.#publish();
      if (committed) {
        try { await this.saved(item.patch); } catch (error) { this.failed(error); }
      }
      item.resolve();
    }
    this.#running = false;
  }
}
