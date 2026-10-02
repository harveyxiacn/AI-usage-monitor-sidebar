<!-- Settings → Position: edge, alignment, offset, monitor. [FRONTEND] -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { getMonitors } from '$lib/api';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { settings } from '$lib/stores/settings.svelte';
  import type { Edge, MonitorInfo, VerticalAlign } from '$lib/types';

  const s = $derived(settings.value);

  let monitors = $state<MonitorInfo[]>([]);
  let error = $state<string | null>(null);

  onMount(() => {
    void getMonitors().then((m) => (monitors = m)).catch((e) => (error = String(e)));
  });

  const EDGES: Edge[] = ['left', 'right', 'top', 'bottom'];
  const ALIGNS: VerticalAlign[] = ['top', 'center', 'bottom'];
  /**
   * `verticalAlign`/`verticalOffset` describe the position *along* the docked
   * edge, so on a top/bottom edge they are horizontal. The wire values stay
   * `top|center|bottom`; only the labels follow the orientation (top → left,
   * bottom → right).
   */
  const alongIsHorizontal = $derived(s.edge === 'top' || s.edge === 'bottom');
  const ALIGN_LABELS: Record<VerticalAlign, string> = { top: 'left', center: 'center', bottom: 'right' };
  const alignLabel = (v: VerticalAlign) =>
    alongIsHorizontal ? `settings.horizontalAlign.${ALIGN_LABELS[v]}` : `settings.verticalAlign.${v}`;
  const num = (e: Event) => Number((e.currentTarget as HTMLInputElement).value);
</script>

<SettingsCard id="position" title={t('settings.position')} reset="position">
  {#if error}<p class="err" role="alert">{t('common.error', { message: error })}</p>{/if}

  <Field label={t('settings.edge')}>
    <select aria-label={t('settings.edge')} class="field" value={s.edge} onchange={(e) => void settings.patch({ edge: e.currentTarget.value as Edge })}>
      {#each EDGES as v (v)}<option value={v}>{tDyn(`settings.edge.${v}`)}</option>{/each}
    </select>
  </Field>

  <Field label={tDyn(alongIsHorizontal ? 'settings.horizontalAlign' : 'settings.verticalAlign')}>
    <select aria-label={tDyn(alongIsHorizontal ? 'settings.horizontalAlign' : 'settings.verticalAlign')} class="field" value={s.verticalAlign} onchange={(e) => void settings.patch({ verticalAlign: e.currentTarget.value as VerticalAlign })}>
      {#each ALIGNS as v (v)}<option value={v}>{tDyn(alignLabel(v))}</option>{/each}
    </select>
  </Field>

  <Field label={tDyn(alongIsHorizontal ? 'settings.horizontalOffset' : 'settings.verticalOffset')}>
    <input
      class="field num"
      type="number"
      step="1"
      value={s.verticalOffset}
      onchange={(e) => void settings.patch({ verticalOffset: Math.round(num(e)) })}
      aria-label={tDyn(alongIsHorizontal ? 'settings.horizontalOffset' : 'settings.verticalOffset')}
    />
  </Field>

  <Field label={t('settings.monitor')}>
    <select aria-label={t('settings.monitor')}
      class="field"
      value={s.monitor ?? ''}
      onchange={(e) => void settings.patch({ monitor: e.currentTarget.value || null })}
    >
      <option value="">{t('settings.monitor.primary')}</option>
      {#each monitors as m (m.name)}
        <option value={m.name}>{m.name} — {m.width}×{m.height}{m.isPrimary ? ' ★' : ''}</option>
      {/each}
    </select>
  </Field>

  <Field label={t('settings.alwaysOnTop')}>
    <Toggle
      checked={s.alwaysOnTop}
      label={t('settings.alwaysOnTop')}
      onchange={(v) => void settings.patch({ alwaysOnTop: v })}
    />
  </Field>
</SettingsCard>
