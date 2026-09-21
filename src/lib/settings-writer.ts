import type { Settings } from './types';

/** Nested fields are merged by the backend; use the same rule for previews. */
export type SettingsPatch = Partial<
  Omit<Settings, 'colors' | 'sizes' | 'thresholds' | 'providers' | 'sidebarItems'>
> & {
  colors?: Partial<Settings['colors']>;
  sizes?: Partial<Settings['sizes']>;
  thresholds?: Partial<Settings['thresholds']>;
  providers?: Record<string, Partial<Settings['providers'][string]>>;
  sidebarItems?: Partial<Settings['sidebarItems']>;
};

export function mergeSettings(base: Settings, patch: SettingsPatch): Settings {
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
