<!--
  Activity heatmap: a GitHub-style calendar of local days, or the same range
  folded into a weekday × hour-of-day punch card. [FRONTEND]

  Colour encodes magnitude, so the ramp is *sequential*: one violet hue in four
  steps, light → dark on the light theme and dark → light on the dark one, with
  a neutral outlined swatch for "no activity" and a hatched one for "cost
  unknown". The lightest step deliberately sits below the 3:1 mark contrast a
  categorical palette would need — that is the sequential rule (the near-zero
  end recedes towards the surface) and the relief channel is the per-cell
  title/aria-label plus the table underneath, which carry every number.

  Plain DOM rather than canvas: a few hundred cells, and each one needs to be a
  real focusable control with its own accessible name.
-->
<script lang="ts">
  import { formatCost, formatInt, formatTokens } from '$lib/format';
  import { calendarWeeks, heatLevel, heatThresholds, metricOf } from '$lib/history';
  import { intlLocale, t } from '$lib/i18n/i18n.svelte';
  import type { CalendarDay, CalendarSlot } from '$lib/types';

  interface Props {
    days: CalendarDay[];
    slots: CalendarSlot[];
    /** the heatmap window, `[from, to)` in ms */
    from: number;
    to: number;
    metric: 'tokens' | 'cost';
    view: 'calendar' | 'punchcard';
    /** the day currently selected as the history range, if any */
    selected?: string | null;
    onpick: (date: string) => void;
  }

  let { days, slots, from, to, metric, view, selected = null, onpick }: Props = $props();

  const fmt = (value: number) => (metric === 'cost' ? formatCost(value) : formatTokens(value));

  const weeks = $derived(calendarWeeks(from, to));
  const byDate = $derived(new Map(days.map((day) => [day.date, day])));
  const bySlot = $derived(new Map(slots.map((slot) => [`${slot.weekday}:${slot.hour}`, slot])));

  /** Levels are quartiles of the *visible* grid, so both views stay readable. */
  const dayThresholds = $derived(heatThresholds(days.map((d) => metricOf(d, metric) ?? 0)));
  const slotThresholds = $derived(heatThresholds(slots.map((s) => metricOf(s, metric) ?? 0)));

  /** 2024-01-01 was a Monday; the calendar's rows start there. */
  const WEEK_ANCHOR = Date.UTC(2024, 0, 1);
  const weekdayNames = $derived.by(() => {
    const format = new Intl.DateTimeFormat(intlLocale(), { weekday: 'short', timeZone: 'UTC' });
    return Array.from({ length: 7 }, (_, i) => format.format(new Date(WEEK_ANCHOR + i * 86_400_000)));
  });

  /** A month label above the first week whose Monday starts a new month. */
  const monthLabels = $derived.by(() => {
    const format = new Intl.DateTimeFormat(intlLocale(), { month: 'short' });
    let previous = '';
    return weeks.map((week) => {
      const first = week.find((date) => date !== null);
      if (!first) return '';
      const label = format.format(new Date(`${first}T00:00:00`));
      if (label === previous) return '';
      previous = label;
      return label;
    });
  });

  const dayLabel = (date: string) =>
    new Intl.DateTimeFormat(intlLocale(), { year: 'numeric', month: 'short', day: 'numeric' })
      .format(new Date(`${date}T00:00:00`));

  interface Cell {
    level: number | 'unknown';
    label: string;
  }

  function dayCell(date: string): Cell {
    const day = byDate.get(date);
    if (!day) return { level: 0, label: t('history.activity.dayEmpty', { date: dayLabel(date) }) };
    const value = metricOf(day, metric);
    if (value == null) {
      // a priced-out day is not an empty day: say so instead of drawing zero
      return { level: 'unknown', label: t('history.activity.day', { date: dayLabel(date), value: formatCost(null), requests: formatInt(day.requests) }) };
    }
    return {
      level: heatLevel(value, dayThresholds),
      label: t('history.activity.day', { date: dayLabel(date), value: fmt(value), requests: formatInt(day.requests) }),
    };
  }

  function slotCell(weekday: number, hour: number): Cell {
    const slot = bySlot.get(`${weekday}:${hour}`);
    const when = { weekday: weekdayNames[weekday], hour: `${String(hour).padStart(2, '0')}:00` };
    if (!slot) return { level: 0, label: t('history.activity.slotEmpty', when) };
    const value = metricOf(slot, metric);
    if (value == null) {
      return { level: 'unknown', label: t('history.activity.slot', { ...when, value: formatCost(null), requests: formatInt(slot.requests) }) };
    }
    return {
      level: heatLevel(value, slotThresholds),
      label: t('history.activity.slot', { ...when, value: fmt(value), requests: formatInt(slot.requests) }),
    };
  }

  const HOURS = Array.from({ length: 24 }, (_, hour) => hour);
</script>

<div class="heat">
  {#if view === 'calendar'}
    <div class="scroll">
      <div class="calendar" style:--weeks={weeks.length}>
        <div class="weekdays" aria-hidden="true">
          {#each weekdayNames as name, i (i)}
            <span class="weekday">{i % 2 === 0 ? name : ''}</span>
          {/each}
        </div>
        <div class="months" aria-hidden="true">
          {#each monthLabels as label, i (i)}<span class="month">{label}</span>{/each}
        </div>
        <div class="grid" role="group" aria-label={t('history.activity.accessible')}>
          {#each weeks as week, w (w)}
            {#each week as date, d (d)}
              {#if date === null}
                <span class="cell outside" aria-hidden="true"></span>
              {:else}
                {@const cell = dayCell(date)}
                <button
                  class="cell day"
                  class:picked={date === selected}
                  data-level={cell.level}
                  title={cell.label}
                  aria-label={cell.label}
                  aria-pressed={date === selected}
                  onclick={() => onpick(date)}
                ></button>
              {/if}
            {/each}
          {/each}
        </div>
      </div>
    </div>
  {:else}
    <div class="scroll">
      <table class="punch">
        <caption class="sr-only">{t('history.activity.accessible')}</caption>
        <thead>
          <tr>
            <td></td>
            {#each HOURS as hour (hour)}
              <th scope="col" class="hour">{hour % 6 === 0 ? String(hour).padStart(2, '0') : ''}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each weekdayNames as name, weekday (weekday)}
            <tr>
              <th scope="row" class="weekday">{name}</th>
              {#each HOURS as hour (hour)}
                {@const cell = slotCell(weekday, hour)}
                <td class="slot" title={cell.label}>
                  <!-- role=img carries the reading; the td itself would not -->
                  <span class="cell" role="img" aria-label={cell.label} data-level={cell.level}></span>
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <footer class="legend">
    <span class="hint">{view === 'calendar' ? t('history.activity.hint') : ''}</span>
    <span class="scale">
      <span class="muted">{t('history.activity.less')}</span>
      {#each [0, 1, 2, 3, 4] as level (level)}
        <span class="cell key" data-level={level} aria-hidden="true"></span>
      {/each}
      <span class="muted">{t('history.activity.more')}</span>
    </span>
  </footer>
</div>

<style>
  /* Sequential ramp — one violet hue, four steps. Dark is the default palette
     (theme.css does the same), light overrides below. Violet keeps the heat
     scale away from both provider accents and from the blue focus ring. */
  .heat {
    --heat-empty: #1f1f26;
    --heat-1: #40338c;
    --heat-2: #5a49c4;
    --heat-3: #7e6bea;
    --heat-4: #ada0ff;
    /* thin marks: a fixed small cell, like every calendar heatmap — the grid
       scrolls rather than stretching a day into a banner on a wide window */
    --cell: 0.8125rem;
    --cell-gap: 0.1875rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 0;
  }

  :global(:root[data-theme='light']) .heat {
    --heat-empty: #e9e9f0;
    --heat-1: #cbbdf7;
    --heat-2: #a48deb;
    --heat-3: #7a5fd6;
    --heat-4: #513497;
  }

  .scroll {
    /* the grid is the only thing allowed to scroll sideways here */
    overflow-x: auto;
    padding-bottom: 0.125rem;
  }

  .calendar {
    display: grid;
    grid-template-columns: auto auto;
    grid-template-rows: auto auto;
    justify-content: start;
    gap: 0.25rem;
  }

  .months {
    grid-column: 2;
    grid-row: 1;
    display: grid;
    grid-template-columns: repeat(var(--weeks), var(--cell));
    gap: var(--cell-gap);
  }

  .month {
    font-size: 0.625rem;
    color: var(--muted);
    white-space: nowrap;
    line-height: 1;
  }

  .weekdays {
    grid-column: 1;
    grid-row: 2;
    display: grid;
    grid-template-rows: repeat(7, var(--cell));
    gap: var(--cell-gap);
    padding-right: 0.25rem;
  }

  .weekday {
    font-size: 0.625rem;
    color: var(--muted);
    line-height: 1;
    display: flex;
    align-items: center;
    white-space: nowrap;
  }

  .grid {
    grid-column: 2;
    grid-row: 2;
    display: grid;
    grid-auto-flow: column;
    grid-template-rows: repeat(7, var(--cell));
    grid-auto-columns: var(--cell);
    gap: var(--cell-gap);
  }

  .cell {
    display: block;
    width: var(--cell);
    height: var(--cell);
    border-radius: 0.1875rem;
    background: var(--heat-empty);
    /* an empty day is an outlined slot; a day outside the window draws nothing */
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .cell[data-level='1'] { background: var(--heat-1); box-shadow: none; }
  .cell[data-level='2'] { background: var(--heat-2); box-shadow: none; }
  .cell[data-level='3'] { background: var(--heat-3); box-shadow: none; }
  .cell[data-level='4'] { background: var(--heat-4); box-shadow: none; }

  /* "cost unknown" is neither zero nor a value: hatch it */
  .cell[data-level='unknown'] {
    background: repeating-linear-gradient(
      45deg,
      var(--heat-empty) 0 2px,
      var(--surface-3) 2px 4px
    );
  }

  .outside {
    background: none;
    box-shadow: none;
  }

  .day {
    padding: 0;
    cursor: pointer;
    transition: transform var(--dur-ui) var(--ease-out);
  }

  .day:hover {
    transform: scale(1.18);
  }

  .day:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    /* the ring must stay above the neighbouring cells it overlaps */
    position: relative;
    z-index: 1;
  }

  .picked {
    box-shadow: inset 0 0 0 1px var(--text);
  }

  .punch {
    border-collapse: separate;
    border-spacing: calc(var(--cell-gap) / 2);
  }

  .punch .hour,
  .punch .weekday {
    font-size: 0.625rem;
    font-weight: 400;
    color: var(--muted);
    text-align: center;
    white-space: nowrap;
  }

  .punch .weekday {
    text-align: right;
    padding-right: 0.25rem;
  }

  .slot {
    padding: 0;
    width: var(--cell);
  }

  .legend {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    flex-wrap: wrap;
  }

  .hint,
  .muted {
    font-size: 0.6875rem;
    color: var(--muted);
  }

  .scale {
    display: inline-flex;
    align-items: center;
    gap: 0.1875rem;
    margin-left: auto;
  }

  .key {
    width: 0.6875rem;
    height: 0.6875rem;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
