<!--
  Root layout for all three windows. Only job: pull in the design system once.
  Each route marks <html data-win="..."> on mount (see $lib/stores/theme.svelte)
  which switches base.css between the transparent widget rules and the opaque
  dashboard rules.
-->
<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

{@render children()}
