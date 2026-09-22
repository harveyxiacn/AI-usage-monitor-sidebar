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
  #reading: number | null = null;
  #subscribing: number | null = null;
  #stopSync: (() => void) | null = null;

  init(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) void this.#start();
    let disposed = false;
    return () => {
      if (disposed) return;
      disposed = true;
      this.#refs -= 1;
      if (this.#refs === 0) {
        this.#generation++;
        this.#stopSync?.();
        this.#stopSync = null;
        this.#unlisten?.();
        this.#unlisten = null;
        this.refreshing = null;
      }
    };
  }

  async #start() {
    const generation = ++this.#generation;
    // Events provide immediate updates. Reconcile with the backend's in-memory
    // snapshot as well: a missed event or failed subscription must not leave a
    // long-lived sidebar showing its startup values forever. No provider API
    // is polled by getSnapshot().
    const sync = () => {
      void this.#subscribe(generation);
      void this.#read(generation);
    };
    const resume = () => {
      if (document.visibilityState !== 'hidden') sync();
    };
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
    // Subscribe first so the initial read cannot miss a concurrent broadcast.
    await this.#subscribe(generation);
    await this.#read(generation);
  }

  async #subscribe(generation: number) {
    if (generation !== this.#generation || this.#unlisten || this.#subscribing === generation) return;
    this.#subscribing = generation;
    try {
      const un = await onSnapshotUpdated((s) => {
        if (generation !== this.#generation) return;
        this.#receive(s);
      });
      if (generation !== this.#generation) { un(); return; }
      this.#unlisten = un;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    } finally {
      if (this.#subscribing === generation) this.#subscribing = null;
    }
  }

  async #read(generation: number) {
    if (generation !== this.#generation || this.#reading === generation || this.refreshing !== null) return;
    this.#reading = generation;
    const revision = this.#revision;
    try {
      const value = await getSnapshot();
      if (generation !== this.#generation) return;
      // A read begun before a manual refresh must not supersede its response.
      if (revision === this.#revision && this.refreshing === null) this.#receive(value);
    } catch (e) {
      if (generation === this.#generation && revision === this.#revision && this.refreshing === null) {
        this.error = String(e);
      }
    } finally {
      if (this.#reading === generation) this.#reading = null;
      if (generation === this.#generation) this.loading = false;
    }
  }

  #receive(value: AppSnapshot) {
    this.#revision++;
    this.value = value;
    this.loading = false;
    this.error = null;
  }

  async refresh(provider?: ProviderId): Promise<void> {
    if (this.refreshing !== null) return;
    this.refreshing = provider ?? 'all';
    const generation = this.#generation;
    const revision = this.#revision;
    try {
      const value = await refreshNow(provider);
      if (generation !== this.#generation) return;
      if (revision === this.#revision) this.#receive(value);
      this.error = null;
    } catch (e) {
      if (generation === this.#generation && revision === this.#revision) this.error = String(e);
    } finally {
      if (generation === this.#generation) this.refreshing = null;
    }
  }
}

export const snapshot = new SnapshotStore();
