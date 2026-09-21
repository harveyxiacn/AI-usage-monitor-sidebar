// Shared AppSnapshot store. [FRONTEND]
import { getSnapshot, onSnapshotUpdated, refreshNow, type Unlisten } from '$lib/api';
import type { AppSnapshot, ProviderId } from '$lib/types';

class SnapshotStore {
  value = $state<AppSnapshot | null>(null);
  loading = $state(true);
  error = $state<string | null>(null);
  /** provider id currently being force-refreshed, or 'all' */
  refreshing = $state<ProviderId | 'all' | null>(null);

  #refs = 0;
  #unlisten: Unlisten | null = null;
  #generation = 0;
  #revision = 0;

  init(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) void this.#start();
    return () => {
      this.#refs -= 1;
      if (this.#refs === 0) {
        this.#generation++;
        this.#unlisten?.();
        this.#unlisten = null;
      }
    };
  }

  async #start() {
    const generation = ++this.#generation;
    try {
      const un = await onSnapshotUpdated((s) => {
        if (generation !== this.#generation) return;
        this.#receive(s);
        this.loading = false;
      });
      if (generation !== this.#generation) { un(); return; }
      this.#unlisten = un;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    }
    const revision = this.#revision;
    try {
      const value = await getSnapshot();
      if (generation !== this.#generation) return;
      if (revision === this.#revision) this.#receive(value);
      this.error = null;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    } finally {
      if (generation === this.#generation) this.loading = false;
    }
  }

  #receive(value: AppSnapshot) {
    this.#revision++;
    this.value = value;
    this.error = null;
  }

  async refresh(provider?: ProviderId): Promise<void> {
    if (this.refreshing !== null) return;
    this.refreshing = provider ?? 'all';
    const revision = this.#revision;
    try {
      const value = await refreshNow(provider);
      if (revision === this.#revision) this.#receive(value);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.refreshing = null;
    }
  }
}

export const snapshot = new SnapshotStore();
