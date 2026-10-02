// Shared state of the Settings tab: the search box and, per card, which of its
// controls match it. [FRONTEND]
//
// `SettingsTab` provides one `SettingsSearch`; every `SettingsCard` provides a
// `CardScope` for its children; `Field` and `SettingsBlock` read both. Outside
// the Settings tab (the analysis settings in Sessions use `Field` too) there is
// no context and everything is simply shown.
import { getContext, setContext } from 'svelte';
import { SvelteMap } from 'svelte/reactivity';
import { normalizeQuery } from './settings-search';
export { controlShown } from './settings-search';
import { advancedSettings } from './stores/advanced.svelte';

export class SettingsSearch {
  query = $state('');
  /** card id → is the card (still) visible? Filled by the cards themselves. */
  cards = new SvelteMap<string, boolean>();
  active = $derived(normalizeQuery(this.query) !== '');
  /** False only when a search is active and no card matches it. */
  anyVisible = $derived(!this.active || [...this.cards.values()].some(Boolean));
  /** "Show advanced settings" is on. */
  showAdvanced = $derived(advancedSettings.on);
  /**
   * Advanced controls are in play: the switch is on, or a search is active (a
   * search always finds them; they then carry an "advanced" badge).
   */
  revealsAdvanced = $derived(advancedSettings.on || this.active);
}

/** What a card tells its controls. */
export interface CardScope {
  /** The card's own title/keywords match the search, so every control shows. */
  readonly headMatches: boolean;
  /** Every control of the card is advanced (see `cardAdvancedOnly`). */
  readonly advanced: boolean;
  /** control → does it show? Controls register here; the card hides when none does. */
  readonly fields: SvelteMap<symbol, boolean>;
}

const SEARCH = Symbol('settings-search');
const SCOPE = Symbol('settings-card-scope');

export function provideSettingsSearch(search: SettingsSearch): void {
  setContext(SEARCH, search);
}
export const getSettingsSearch = (): SettingsSearch | undefined => getContext<SettingsSearch | undefined>(SEARCH);
export function provideCardScope(scope: CardScope): void {
  setContext(SCOPE, scope);
}
export const getCardScope = (): CardScope | undefined => getContext<CardScope | undefined>(SCOPE);
