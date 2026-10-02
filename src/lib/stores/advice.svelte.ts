// Routing advice store: `get_routing_advice` re-read whenever the snapshot
// changes. One instance per window; the backend computes it from the in-memory
// snapshot only (no I/O, nothing is polled). [FRONTEND]
import { getRoutingAdvice } from '$lib/api';
import type { RoutingAdvice } from '$lib/types';

class AdviceStore {
  /** null = nothing worth saying (or not loaded yet) */
  value = $state<RoutingAdvice | null>(null);
  #request = 0;

  /** Call from an `$effect` that depends on the snapshot's `generatedAt`. */
  async refresh(): Promise<void> {
    const id = ++this.#request;
    try {
      const next = await getRoutingAdvice();
      if (id === this.#request) this.value = next;
    } catch {
      // the advice is a convenience; a failure only hides it
      if (id === this.#request) this.value = null;
    }
  }

  /** Invalidate in-flight reads (component teardown). */
  stop(): void {
    this.#request++;
  }
}

export const advice = new AdviceStore();
