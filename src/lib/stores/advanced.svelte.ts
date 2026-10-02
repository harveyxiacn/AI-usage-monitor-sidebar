// "Show advanced settings": one switch for the whole Settings tab, kept per
// viewer in localStorage (it is UI state, not app behaviour, so it is not in
// settings.json). [FRONTEND]
import { loadShowAdvanced, saveShowAdvanced } from '../settings-tiers';

class AdvancedSettings {
  on = $state(typeof window === 'undefined' ? false : loadShowAdvanced());

  set(next: boolean): void {
    this.on = next;
    saveShowAdvanced(next);
  }
}

export const advancedSettings = new AdvancedSettings();
