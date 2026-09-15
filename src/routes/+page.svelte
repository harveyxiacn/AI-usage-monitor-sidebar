<!-- window "sidebar" — placeholder, replaced by the frontend implementation -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { getSnapshot } from '$lib/api';
  import type { AppSnapshot } from '$lib/types';
  let snapshot: AppSnapshot | null = $state(null);
  onMount(async () => { snapshot = await getSnapshot(); });
</script>
<main style="color:#fff;background:#111;border-radius:24px;padding:12px;font:13px system-ui">
  {#if snapshot}
    {#each snapshot.providers as p}
      <div>{p.displayName}: {p.windows.find(w => w.isPrimary)?.usedPercent ?? '–'}%</div>
    {/each}
  {:else}
    loading…
  {/if}
</main>
