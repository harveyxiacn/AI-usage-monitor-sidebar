// Static facts for the "Privacy & network" card: what the app reads on this
// machine and every outbound request it can make, each tied to the setting
// that controls it. [FRONTEND]
//
// The texts live in the i18n catalogues under `privacy.item.<id>.*`. When the
// backend gains a request, it must gain a row here: `tests/privacy-network.unit.ts`
// keeps the rows and the catalogues in step.
import type { Settings } from './types';

export type PrivacyItemId =
  | 'credentials'
  | 'sessionLogs'
  | 'ownFiles'
  | 'snapshot'
  | 'claude'
  | 'codex'
  | 'copilot'
  | 'update'
  | 'pricing'
  | 'assessment'
  | 'webhook';

export interface PrivacyItem {
  id: PrivacyItemId;
  kind: 'local' | 'request';
  /** the setting that switches it off; null = always on (local) or user-initiated only */
  setting: keyof Settings | null;
  /** for `providers`: which provider's `enabled` flag applies */
  provider?: string;
}

export const PRIVACY_ITEMS: readonly PrivacyItem[] = [
  { id: 'credentials', kind: 'local', setting: 'providers' },
  { id: 'sessionLogs', kind: 'local', setting: 'ingestEnabled' },
  { id: 'ownFiles', kind: 'local', setting: null },
  { id: 'snapshot', kind: 'local', setting: 'exportSnapshot' },
  { id: 'claude', kind: 'request', setting: 'providers', provider: 'claude' },
  { id: 'codex', kind: 'request', setting: 'providers', provider: 'codex' },
  { id: 'copilot', kind: 'request', setting: 'providers', provider: 'copilot' },
  { id: 'update', kind: 'request', setting: 'autoUpdateCheck' },
  { id: 'pricing', kind: 'request', setting: 'autoPricingCheck' },
  { id: 'assessment', kind: 'request', setting: null },
  { id: 'webhook', kind: 'request', setting: 'webhook' },
];

/**
 * Is this item currently active? `null` when it only ever happens on an
 * explicit user action (nothing to switch off).
 */
export function privacyItemActive(item: PrivacyItem, s: Settings): boolean | null {
  switch (item.setting) {
    case null:
      return null;
    case 'providers':
      return item.provider
        ? (s.providers[item.provider]?.enabled ?? false)
        : Object.values(s.providers).some((p) => p.enabled);
    case 'ingestEnabled':
      return s.ingestEnabled;
    case 'autoUpdateCheck':
      return s.autoUpdateCheck;
    case 'autoPricingCheck':
      return s.autoPricingCheck;
    case 'exportSnapshot':
      return s.exportSnapshot;
    case 'webhook':
      // alerts reach the webhook only while notifications are on
      return s.notifications && s.webhook.enabled && s.webhook.url !== '';
    default:
      return null;
  }
}
