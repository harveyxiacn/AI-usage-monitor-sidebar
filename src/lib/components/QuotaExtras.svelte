<!--
  The provider-neutral `ProviderQuota.extras` as a compact chip row. [FRONTEND]

  The backend only ships a stable `kind` plus already-formatted values, so the
  label comes from `extras.<kind>` here and an unknown future kind degrades to
  its raw id instead of an "extras.…" placeholder. `detail` (a model list, the
  user's own spend cap, …) stays in the title so the row keeps one line.

  Renders nothing when the list is empty — every caller can mount it
  unconditionally.
-->
<script lang="ts">
  import { hasKey, t, tDyn } from '$lib/i18n/i18n.svelte';
  import type { QuotaExtra } from '$lib/types';

  interface Props {
    extras: QuotaExtra[];
    /** `popover` is tighter and has no heading; `dashboard` gets one. */
    context?: 'popover' | 'dashboard';
  }

  let { extras, context = 'popover' }: Props = $props();

  const label = (kind: string) => (hasKey(`extras.${kind}`) ? tDyn(`extras.${kind}`) : kind);

  function title(e: QuotaExtra): string {
    const name = label(e.kind);
    if (e.detail) return t('extras.detail', { label: name, value: e.value ?? '', detail: e.detail });
    return e.value ? `${name}: ${e.value}` : name;
  }
</script>

{#if extras.length > 0}
  <div class="extras" data-context={context}>
    {#if context === 'dashboard'}
      <span class="heading">{t('extras.title')}</span>
    {/if}
    <ul>
      {#each extras as e, i (e.kind + ':' + i)}
        <li class="chip" data-severity={e.severity} title={title(e)}>
          <span class="k">{label(e.kind)}</span>
          {#if e.value}<span class="v">{e.value}</span>{/if}
        </li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .extras {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }

  .extras[data-context='popover'] {
    padding-top: 0.125rem;
  }

  .heading {
    flex: none;
    font-size: 0.6875rem;
    color: var(--faint);
    white-space: nowrap;
  }

  ul {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3125rem;
    margin: 0;
    padding: 0;
    list-style: none;
    min-width: 0;
  }

  /* the chip borrows the popover's own tokens, so glass / solid / cyber and
     dark / light all stay consistent without a per-style rule */
  .chip {
    display: inline-flex;
    align-items: baseline;
    gap: 0.3125rem;
    max-width: 100%;
    padding: 0.0625rem 0.375rem;
    border: 1px solid var(--bar-border);
    border-radius: 999px;
    background: var(--hover);
    font-size: 0.6875rem;
    line-height: 1.5;
  }

  .k {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .v {
    font-variant-numeric: tabular-nums;
    font-weight: 500;
  }

  .chip[data-severity='warn'] {
    border-color: color-mix(in srgb, var(--warn) 55%, transparent);
  }

  .chip[data-severity='warn'] .k,
  .chip[data-severity='warn'] .v {
    color: var(--warn);
  }

  .chip[data-severity='critical'] {
    border-color: color-mix(in srgb, var(--critical) 60%, transparent);
  }

  .chip[data-severity='critical'] .k,
  .chip[data-severity='critical'] .v {
    color: var(--critical);
  }
</style>
