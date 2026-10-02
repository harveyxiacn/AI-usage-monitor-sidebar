<!--
  A compact "label: before → after" list for the preset preview, the import
  result and the undo history. Labels come from the settings i18n keys when
  there is one for the path, otherwise the dotted path itself. [FRONTEND]
-->
<script lang="ts">
  import { hasKey, t, tDyn } from '$lib/i18n/i18n.svelte';
  import { formatSettingValue, type SettingChange } from '$lib/settings-diff';

  interface Props {
    changes: SettingChange[];
  }

  let { changes }: Props = $props();

  /** Paths whose i18n key is not simply `settings.<path>`. */
  const SPECIAL: Record<string, string> = {
    'thresholds.warn': 'settings.warnThreshold',
    'thresholds.critical': 'settings.criticalThreshold',
    verticalAlign: 'settings.verticalAlign',
    verticalOffset: 'settings.verticalOffset',
  };

  function label(path: string): string {
    const direct = SPECIAL[path] ?? `settings.${path}`;
    if (hasKey(direct)) return tDyn(direct);
    // providers.codex.enabled → "Track this provider" for the codex row
    const provider = /^providers\.([^.]+)\.(enabled|showInSidebar)$/.exec(path);
    if (provider) {
      const key = provider[2] === 'enabled' ? 'settings.providerEnabled' : 'settings.sidebarItems.provider';
      return `${provider[1]} — ${tDyn(key)}`;
    }
    const top = path.split('.')[0];
    return hasKey(`settings.${top}`) ? `${tDyn(`settings.${top}`)} › ${path.slice(top.length + 1)}` : path;
  }

  /** Enum values have their own translations (`settings.theme.dark`); others print as they are. */
  function value(path: string, v: unknown): string {
    if (typeof v === 'string') {
      const key = `settings.${path}.${v}`;
      if (hasKey(key)) return tDyn(key);
    }
    return formatSettingValue(v);
  }
</script>

{#if changes.length === 0}
  <p class="muted small">{t('settings.changes.none')}</p>
{:else}
  <ul class="changes">
    {#each changes as c (c.path)}
      <li>
        <span class="what">{label(c.path)}</span>
        <span class="delta"><span class="was">{value(c.path, c.before)}</span> → <span class="now">{value(c.path, c.after)}</span></span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .changes {
    list-style: none;
    margin: 0.25rem 0;
    padding: 0;
    font-size: 0.75rem;
  }

  li {
    display: flex;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.125rem 0;
  }

  .what {
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .delta {
    text-align: right;
    overflow-wrap: anywhere;
    font-variant-numeric: tabular-nums;
  }

  .was {
    color: var(--faint);
  }

  .now {
    color: var(--text);
    font-weight: 500;
  }
</style>
