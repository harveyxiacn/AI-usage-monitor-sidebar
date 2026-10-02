// The shareable usage card: what it says, where things sit, how it is drawn. [FRONTEND]
//
// Three steps so the logic is testable without a canvas:
//   1. `buildShareModel`  - what the card says (rings, month totals, top model).
//                           No account e-mail or name ever enters the model.
//   2. `layoutShareCard`  - pure geometry on a 1200x630 canvas.
//   3. `drawShareCard`    - paints the model into a 2D context.
// Rune-free and i18n-free: strings arrive pre-translated in `ShareLabels`.
import { clampPercent, severityOf, type Severity } from './severity';
import type { HistoryRow, PercentMode, ProviderId, ProviderQuota, QuotaWindow, Settings, Thresholds, TokenTotals } from './types';

export const CARD_WIDTH = 1200;
export const CARD_HEIGHT = 630;

export interface ShareLabels {
  /** "AI usage" */
  title: string;
  /** "This month" */
  thisMonth: string;
  tokens: string;
  cost: string;
  /** "estimate": the cost is never an invoice */
  estimate: string;
  topModel: string;
  /** The app name in the footer. */
  appName: string;
  /** Footer note beside the cost, e.g. "Estimated from list prices, not billing". */
  costNote: string;
  /** Shown instead of rings when no provider has data. */
  noData: string;
}

export interface ShareFormat {
  tokens: (n: number | null | undefined) => string;
  cost: (usd: number | null | undefined) => string;
  /** The text inside a ring for a used percentage, in the user's percent mode. */
  percent: (used: number) => string;
}

export interface ShareInput {
  providers: readonly ProviderQuota[];
  providerSettings: Settings['providers'];
  thresholds: Thresholds;
  percentMode: PercentMode;
  /** Concrete colour (hex) per provider; the canvas cannot resolve `var()`. */
  accentOf: (provider: ProviderId) => string;
  /** The month the totals are for, e.g. "October 2026". */
  monthLabel: string;
  totals: TokenTotals | null;
  rows: readonly HistoryRow[];
  hideCost: boolean;
  theme: 'dark' | 'light';
  labels: ShareLabels;
  format: ShareFormat;
}

export interface ShareRing {
  provider: ProviderId;
  name: string;
  windowLabel: string;
  /** Used percentage 0..100: how far the arc runs. */
  fill: number;
  /** Text in the middle ("73%" or "27% left"). */
  text: string;
  color: string;
  severity: Severity;
}

export interface ShareStat {
  id: 'tokens' | 'cost' | 'topModel';
  label: string;
  value: string;
  /** Small secondary line ("estimate", "42% of tokens"). */
  note: string | null;
}

export interface ShareModel {
  theme: 'dark' | 'light';
  labels: ShareLabels;
  monthLabel: string;
  rings: ShareRing[];
  stats: ShareStat[];
  hideCost: boolean;
}

export interface SharePalette {
  bg: string;
  panel: string;
  text: string;
  muted: string;
  track: string;
  warn: string;
  critical: string;
}

export const PALETTES: Record<'dark' | 'light', SharePalette> = {
  dark: { bg: '#0e1014', panel: '#171a21', text: '#f2f4f8', muted: '#9aa3b2', track: '#2b303b', warn: '#f5c542', critical: '#ff453a' },
  light: { bg: '#eef0f4', panel: '#ffffff', text: '#14161a', muted: '#5b6473', track: '#e0e4eb', warn: '#c98a00', critical: '#d62c20' },
};

/** The model with the most tokens in `rows` (rows without a model are skipped). */
export function topModel(rows: readonly HistoryRow[]): { name: string; tokens: number; share: number } | null {
  const by = new Map<string, number>();
  let total = 0;
  for (const row of rows) {
    total += row.totalTokens;
    if (!row.model) continue;
    by.set(row.model, (by.get(row.model) ?? 0) + row.totalTokens);
  }
  let best: { name: string; tokens: number } | null = null;
  for (const [name, tokens] of by) if (!best || tokens > best.tokens) best = { name, tokens };
  return best && best.tokens > 0 ? { ...best, share: total > 0 ? best.tokens / total : 0 } : null;
}

/** The window a provider's ring stands for: the primary one, else the busiest. */
function headline(windows: readonly QuotaWindow[]): QuotaWindow | null {
  return windows.find((w) => w.isPrimary) ?? [...windows].sort((a, b) => b.usedPercent - a.usedPercent)[0] ?? null;
}

export function buildShareModel(input: ShareInput): ShareModel {
  const { labels, format } = input;
  const rings: ShareRing[] = [];
  const ordered = [...input.providers].sort(
    (a, b) => (input.providerSettings[a.provider]?.order ?? 0) - (input.providerSettings[b.provider]?.order ?? 0),
  );
  for (const q of ordered) {
    if (input.providerSettings[q.provider]?.enabled === false) continue;
    if (q.status !== 'ok' && q.status !== 'rate_limited') continue;
    const w = headline(q.windows);
    if (!w) continue;
    const used = clampPercent(w.usedPercent);
    rings.push({
      provider: q.provider,
      // only the display name: never the account, plan e-mail or any id of the user
      name: q.displayName,
      windowLabel: w.label,
      fill: used,
      text: format.percent(used),
      color: input.accentOf(q.provider),
      severity: severityOf(used, input.thresholds),
    });
  }

  const stats: ShareStat[] = [];
  const totals = input.totals;
  stats.push({ id: 'tokens', label: labels.tokens, value: format.tokens(totals?.totalTokens), note: null });
  if (!input.hideCost) {
    const usd = totals ? (totals.estimatedCostUsd ?? totals.knownCostUsd ?? null) : null;
    stats.push({ id: 'cost', label: labels.cost, value: format.cost(usd), note: labels.estimate });
  }
  const top = topModel(input.rows);
  if (top) {
    stats.push({ id: 'topModel', label: labels.topModel, value: top.name, note: `${Math.round(top.share * 100)}% · ${format.tokens(top.tokens)}` });
  }
  return { theme: input.theme, labels, monthLabel: input.monthLabel, rings, stats, hideCost: input.hideCost };
}

// ---------- layout ----------

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface RingSlot {
  cx: number;
  cy: number;
  /** Radius of the arc's centre line. */
  r: number;
  stroke: number;
  /** Baselines of the provider name and the window label under the ring. */
  nameY: number;
  windowY: number;
  /** Font size of the number inside the ring. */
  valueSize: number;
}

export interface StatSlot extends Box {
  labelY: number;
  valueY: number;
  noteY: number;
  valueSize: number;
}

export interface ShareLayout {
  width: number;
  height: number;
  pad: number;
  header: { titleX: number; monthX: number; y: number };
  /** Where the rings live and where the stats live. */
  ringsArea: Box;
  statsArea: Box;
  rings: RingSlot[];
  stats: StatSlot[];
  footer: { x: number; rightX: number; y: number };
}

const PAD = 56;
const HEADER_BOTTOM = 112;
const FOOTER_HEIGHT = 84;
const RING_GAP = 28;
const RING_LABEL_HEIGHT = 58;
const MAX_PER_ROW = 4;

export function layoutShareCard(model: Pick<ShareModel, 'rings' | 'stats'>, width = CARD_WIDTH, height = CARD_HEIGHT): ShareLayout {
  const bodyTop = HEADER_BOTTOM;
  const bodyH = height - HEADER_BOTTOM - FOOTER_HEIGHT;
  const hasStats = model.stats.length > 0;
  const statsW = hasStats ? 380 : 0;
  const ringsArea: Box = { x: PAD, y: bodyTop, w: width - PAD * 2 - (hasStats ? statsW + 48 : 0), h: bodyH };
  const statsArea: Box = { x: width - PAD - statsW, y: bodyTop, w: statsW, h: bodyH };

  const n = model.rings.length;
  const rings: RingSlot[] = [];
  if (n > 0) {
    const perRow = Math.min(n, MAX_PER_ROW);
    const rows = Math.ceil(n / perRow);
    const byWidth = (ringsArea.w - RING_GAP * (perRow - 1)) / perRow;
    const byHeight = (ringsArea.h - rows * RING_LABEL_HEIGHT - RING_GAP * (rows - 1)) / rows;
    const diameter = Math.max(60, Math.min(220, byWidth, byHeight));
    const stroke = Math.max(8, Math.round(diameter * 0.09));
    const blockH = diameter + RING_LABEL_HEIGHT;
    const usedH = rows * blockH + (rows - 1) * RING_GAP;
    const top = ringsArea.y + Math.max(0, (ringsArea.h - usedH) / 2);
    for (let i = 0; i < n; i++) {
      const row = Math.floor(i / perRow);
      const inRow = Math.min(perRow, n - row * perRow);
      const rowW = inRow * diameter + (inRow - 1) * RING_GAP;
      const left = ringsArea.x + (ringsArea.w - rowW) / 2;
      const col = i - row * perRow;
      const cx = left + col * (diameter + RING_GAP) + diameter / 2;
      const cy = top + row * (blockH + RING_GAP) + diameter / 2;
      rings.push({
        cx,
        cy,
        r: diameter / 2 - stroke / 2,
        stroke,
        nameY: cy + diameter / 2 + 26,
        windowY: cy + diameter / 2 + 50,
        valueSize: Math.round(diameter * 0.22),
      });
    }
  }

  const stats: StatSlot[] = [];
  const count = model.stats.length;
  if (count > 0) {
    const slotH = statsArea.h / count;
    for (let i = 0; i < count; i++) {
      const y = statsArea.y + i * slotH;
      stats.push({
        x: statsArea.x,
        y,
        w: statsArea.w,
        h: slotH,
        labelY: y + slotH * 0.2,
        valueY: y + slotH * 0.62,
        noteY: y + slotH * 0.86,
        valueSize: count >= 3 ? 52 : 64,
      });
    }
  }

  return {
    width,
    height,
    pad: PAD,
    header: { titleX: PAD, monthX: width - PAD, y: 68 },
    ringsArea,
    statsArea,
    rings,
    stats,
    footer: { x: PAD, rightX: width - PAD, y: height - 40 },
  };
}

/**
 * The largest font size (from `start` down to `min`) at which `text` fits
 * `maxWidth`, given a width function for a size. Used for long model names.
 */
export function fitFontSize(widthAt: (size: number) => number, maxWidth: number, start: number, min: number): number {
  let size = start;
  while (size > min && widthAt(size) > maxWidth) size -= 2;
  return Math.max(min, size);
}

// ---------- drawing ----------

const FONT = 'system-ui, -apple-system, "Segoe UI", "Helvetica Neue", Arial, "Noto Sans CJK SC", "Microsoft YaHei", sans-serif';

function roundedRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

function ellipsize(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string {
  if (ctx.measureText(text).width <= maxWidth) return text;
  let cut = text;
  while (cut.length > 1 && ctx.measureText(`${cut}…`).width > maxWidth) cut = cut.slice(0, -1);
  return `${cut}…`;
}

/** Paint `model` into `ctx` (a 1200x630 canvas context) using `layout`. */
export function drawShareCard(ctx: CanvasRenderingContext2D, model: ShareModel, layout: ShareLayout, colors?: { warn?: string; critical?: string }): void {
  const p = { ...PALETTES[model.theme] };
  if (colors?.warn) p.warn = colors.warn;
  if (colors?.critical) p.critical = colors.critical;
  const { width, height } = layout;

  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = p.bg;
  ctx.fillRect(0, 0, width, height);
  ctx.fillStyle = p.panel;
  roundedRect(ctx, 20, 20, width - 40, height - 40, 28);
  ctx.fill();

  ctx.textBaseline = 'alphabetic';
  ctx.textAlign = 'left';
  ctx.fillStyle = p.text;
  ctx.font = `700 34px ${FONT}`;
  ctx.fillText(model.labels.title, layout.header.titleX, layout.header.y);
  ctx.textAlign = 'right';
  ctx.fillStyle = p.muted;
  ctx.font = `500 24px ${FONT}`;
  ctx.fillText(`${model.labels.thisMonth} · ${model.monthLabel}`, layout.header.monthX, layout.header.y);

  // rings
  if (model.rings.length === 0) {
    ctx.textAlign = 'center';
    ctx.fillStyle = p.muted;
    ctx.font = `500 26px ${FONT}`;
    ctx.fillText(model.labels.noData, layout.ringsArea.x + layout.ringsArea.w / 2, layout.ringsArea.y + layout.ringsArea.h / 2);
  }
  model.rings.forEach((ring, i) => {
    const slot = layout.rings[i];
    const color = ring.severity === 'critical' ? p.critical : ring.severity === 'warn' ? p.warn : ring.color;
    ctx.lineWidth = slot.stroke;
    ctx.lineCap = 'round';
    ctx.strokeStyle = p.track;
    ctx.beginPath();
    ctx.arc(slot.cx, slot.cy, slot.r, 0, Math.PI * 2);
    ctx.stroke();
    if (ring.fill > 0) {
      ctx.strokeStyle = color;
      ctx.beginPath();
      const start = -Math.PI / 2;
      ctx.arc(slot.cx, slot.cy, slot.r, start, start + (Math.PI * 2 * Math.min(100, ring.fill)) / 100);
      ctx.stroke();
    }
    ctx.textAlign = 'center';
    ctx.fillStyle = p.text;
    ctx.font = `700 ${slot.valueSize}px ${FONT}`;
    ctx.fillText(ring.text, slot.cx, slot.cy + slot.valueSize * 0.35, slot.r * 1.7);
    const maxLabel = slot.r * 2 + 24;
    ctx.font = `600 22px ${FONT}`;
    ctx.fillText(ellipsize(ctx, ring.name, maxLabel), slot.cx, slot.nameY);
    ctx.fillStyle = p.muted;
    ctx.font = `500 18px ${FONT}`;
    ctx.fillText(ellipsize(ctx, ring.windowLabel, maxLabel), slot.cx, slot.windowY);
  });

  // stats
  model.stats.forEach((stat, i) => {
    const slot = layout.stats[i];
    if (i > 0) {
      ctx.fillStyle = p.track;
      ctx.fillRect(slot.x, slot.y, slot.w, 2);
    }
    ctx.textAlign = 'left';
    ctx.fillStyle = p.muted;
    ctx.font = `600 20px ${FONT}`;
    ctx.fillText(stat.label.toLocaleUpperCase(), slot.x, slot.labelY);
    ctx.fillStyle = p.text;
    const size = fitFontSize((s) => {
      ctx.font = `700 ${s}px ${FONT}`;
      return ctx.measureText(stat.value).width;
    }, slot.w, slot.valueSize, 24);
    ctx.font = `700 ${size}px ${FONT}`;
    ctx.fillText(stat.value, slot.x, slot.valueY, slot.w);
    if (stat.note) {
      ctx.fillStyle = p.muted;
      ctx.font = `500 20px ${FONT}`;
      ctx.fillText(stat.note, slot.x, slot.noteY, slot.w);
    }
  });

  // footer
  ctx.textAlign = 'left';
  ctx.fillStyle = p.muted;
  ctx.font = `600 20px ${FONT}`;
  ctx.fillText(model.labels.appName, layout.footer.x, layout.footer.y);
  if (!model.hideCost) {
    ctx.textAlign = 'right';
    ctx.font = `500 18px ${FONT}`;
    ctx.fillText(model.labels.costNote, layout.footer.rightX, layout.footer.y);
  }
}

/** File name for the saved image: `ai-usage-2026-10.png`. */
export function shareFileName(now: number): string {
  const d = new Date(now);
  return `ai-usage-${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}.png`;
}

/** Local-time bounds `[from, to)` of the month containing `now`. */
export function monthBounds(now: number): { from: number; to: number } {
  const d = new Date(now);
  return { from: new Date(d.getFullYear(), d.getMonth(), 1).getTime(), to: new Date(d.getFullYear(), d.getMonth() + 1, 1).getTime() };
}
