<!--
  Sessions → Insights: one ranked list. Each row opens the session detail and
  carries a warning chip when it crosses an efficiency threshold. [FRONTEND]
-->
<script lang="ts">
  import { projectName } from '$lib/history';
  import { providerDisplayName } from '$lib/providers';
  import { st } from '$lib/session-labels.svelte';
  import type { InsightSession, InsightThresholds } from '$lib/session-types';

  interface Props {
    title: string;
    rows: InsightSession[];
    /** the ranked value, already formatted */
    value: (row: InsightSession) => string;
    /** secondary facts under the title */
    detail?: (row: InsightSession) => string;
    thresholds: InsightThresholds;
    /** which warning this list is about, if any: the reason is spelled out once */
    explain?: 'failures' | 'repeats';
    onopen: (provider: string, sessionId: string) => void;
  }

  let { title, rows, value, detail, thresholds, explain, onopen }: Props = $props();

  const pct = (v: number) => Math.round(v * 100);
  const why = (flag: 'failures' | 'repeats') =>
    flag === 'failures'
      ? st('whyFailures', { n: thresholds.minFailures, p: pct(thresholds.failureRate) })
      : st('whyRepeats', { n: thresholds.minRepeats, p: pct(thresholds.repeatRate) });
</script>

<section class="top card" aria-label={title}>
  <h4>{title}</h4>
  {#if explain}<p class="muted small why">{why(explain)}</p>{/if}
  {#if rows.length === 0}
    <p class="muted small">{st('topNone')}</p>
  {:else}
    <ol>
      {#each rows as row, i (`${row.provider}\u0000${row.sessionId}`)}
        <li>
          <button class="row" onclick={() => onopen(row.provider, row.sessionId)} aria-label={`${st('openSession')}: ${row.title}`}>
            <span class="rank mono" aria-hidden="true">{i + 1}</span>
            <span class="main">
              <strong>{row.title}</strong>
              <span class="muted small">{providerDisplayName(row.provider)} · {projectName(row.project)}{detail ? ` · ${detail(row)}` : ''}</span>
              {#if row.flags.length}
                <span class="chips">
                  {#each row.flags as flag (flag)}
                    <span class="chip" title={why(flag)}><span aria-hidden="true">⚠</span> {flag === 'failures' ? st('flagFailures') : st('flagRepeats')}</span>
                  {/each}
                </span>
              {/if}
            </span>
            <span class="value mono">{value(row)}</span>
          </button>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .top { padding: 0.9rem; display: grid; gap: 0.5rem; min-width: 0; align-content: start; }
  h4 { margin: 0; font-size: 0.85rem; }
  p { margin: 0; }
  .small { font-size: 0.72rem; line-height: 1.5; }
  ol { list-style: none; margin: 0; padding: 0; display: grid; }
  li + li { border-top: 1px solid var(--border); }
  .row { width: 100%; display: grid; grid-template-columns: 1.25rem minmax(0, 1fr) auto; gap: 0.6rem; align-items: start; text-align: left; padding: 0.55rem 0.25rem; border-radius: var(--r-control); }
  .row:hover { background: var(--hover); }
  .rank { color: var(--muted); font-size: 0.72rem; padding-top: 0.15rem; }
  .main { display: grid; gap: 0.2rem; min-width: 0; }
  .main strong { font-size: 0.82rem; overflow-wrap: anywhere; line-height: 1.35; }
  .value { font-size: 0.82rem; font-weight: 600; white-space: nowrap; }
  .chips { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .chip { display: inline-flex; gap: 0.25rem; align-items: center; font-size: 0.68rem; padding: 0.1rem 0.45rem; border-radius: 999px; border: 1px solid var(--warn); color: var(--warn); }
</style>
