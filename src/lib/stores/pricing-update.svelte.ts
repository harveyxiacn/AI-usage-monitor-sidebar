// Shared pricing-update status. Pricing releases are checked and applied
// separately from application releases, so a new price table never implies a
// program download or restart.
import {
  applyPriceUpdate,
  checkForPriceUpdates,
  getPriceUpdateStatus,
  onPriceUpdateStatus,
  type Unlisten,
} from '$lib/api';
import type { PricingTable, PriceUpdateStatus } from '$lib/types';

class PricingUpdateStore {
  value = $state<PriceUpdateStatus | null>(null);
  error = $state<string | null>(null);

  #refs = 0;
  #unlisten: Unlisten | null = null;
  #generation = 0;

  get available(): boolean {
    return this.value?.available ?? false;
  }

  /** Idempotent; every dashboard/settings consumer gets the same status. */
  init(): () => void {
    this.#refs += 1;
    if (this.#refs === 1) void this.#start();
    return () => {
      this.#refs -= 1;
      if (this.#refs === 0) {
        this.#generation += 1;
        this.#unlisten?.();
        this.#unlisten = null;
      }
    };
  }

  async #start(): Promise<void> {
    const generation = ++this.#generation;
    try {
      const unlisten = await onPriceUpdateStatus((status) => {
        if (generation === this.#generation) this.value = status;
      });
      if (generation !== this.#generation) { unlisten(); return; }
      this.#unlisten = unlisten;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    }
    try {
      const status = await getPriceUpdateStatus();
      if (generation === this.#generation) this.value = status;
    } catch (e) {
      if (generation === this.#generation) this.error = String(e);
    }
  }

  async check(): Promise<void> {
    if (this.value?.checking) return;
    this.error = null;
    if (this.value) this.value = { ...this.value, checking: true };
    try {
      this.value = await checkForPriceUpdates();
    } catch (e) {
      this.error = String(e);
      if (this.value) this.value = { ...this.value, checking: false };
    }
  }

  /** Applies a checked table. Callers decide whether it is safe to replace a local draft. */
  async apply(): Promise<PricingTable | null> {
    if (this.value?.applying || this.value?.customPricing) return null;
    this.error = null;
    if (this.value) this.value = { ...this.value, applying: true };
    try {
      return await applyPriceUpdate();
    } catch (e) {
      this.error = String(e);
      return null;
    } finally {
      // The backend event supplies the final status; this only unblocks an
      // older backend that has not emitted it yet.
      if (this.value?.applying) this.value = { ...this.value, applying: false };
    }
  }
}

export const pricingUpdate = new PricingUpdateStore();
