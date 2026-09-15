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
      this.value = await getSnapshot();
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
    const un = await onSnapshotUpdated((s) => {
      this.value = s;
      this.loading = false;
    });
    if (this.#refs === 0) un();
    else this.#unlisten = un;
  }

  async refresh(provider?: ProviderId): Promise<void> {
    this.refreshing = provider ?? 'all';
    try {
      this.value = await refreshNow(provider);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.refreshing = null;
    }
  }
}

export const snapshot = new SnapshotStore();
