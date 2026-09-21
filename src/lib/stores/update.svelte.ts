// Shared UpdateStatus store. [FRONTEND]
//
// The backend never installs anything by itself; this store only mirrors what
// it found and turns the two user actions ("Check for updates", "Install and
// restart") into commands. See docs/RELEASING.md.
import { checkForUpdates, getUpdateStatus, installUpdate, onUpdateStatus, type Unlisten } from '$lib/api';
import type { UpdateStatus } from '$lib/types';

class UpdateStore {
  value = $state<UpdateStatus | null>(null);
  error = $state<string | null>(null);

  #refs = 0;
  #unlisten: Unlisten | null = null;
  #generation = 0;

  /** True while a newer version is on offer. */
  get available(): string | null {
    return this.value?.available ?? null;
  }

  /** Idempotent; returns a disposer to call from onMount cleanup. */
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
      const un = await onUpdateStatus((status) => {
        if (generation !== this.#generation) return;
        this.value = status;
      });
      if (generation !== this.#generation) { un(); return; }
      this.#unlisten = un;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    }
    try {
      const status = await getUpdateStatus();
      if (generation === this.#generation) this.value = status;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    }
  }

  async check(): Promise<void> {
    if (this.value?.checking) return;
    this.error = null;
    // Optimistic, so the button reads "Checking…" before the round trip.
    if (this.value) this.value = { ...this.value, checking: true };
    try {
      this.value = await checkForUpdates();
    } catch (e) {
      this.error = String(e);
      if (this.value) this.value = { ...this.value, checking: false };
    }
  }

  /** Only offered when `canInstall`; the app restarts on success. */
  async install(): Promise<void> {
    this.error = null;
    try {
      await installUpdate();
    } catch (e) {
      this.error = String(e);
    }
  }
}

export const update = new UpdateStore();
