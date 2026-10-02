<!--
  Settings → Privacy & network: the screen-sharing switch plus a static, honest
  list of what the app reads on this machine and every request it can make,
  each with the setting that controls it. The rows are `$lib/privacy-network`;
  the texts are `privacy.item.<id>.*`. [FRONTEND]
-->
<script lang="ts">
  import Field from '../Field.svelte';
  import Toggle from '../Toggle.svelte';
  import SettingsBlock from './SettingsBlock.svelte';
  import SettingsCard from './SettingsCard.svelte';
  import { t, tDyn } from '$lib/i18n/i18n.svelte';
  import { PRIVACY_ITEMS, privacyItemActive, type PrivacyItem } from '$lib/privacy-network';
  import { settings } from '$lib/stores/settings.svelte';

  const s = $derived(settings.value);

  const text = (item: PrivacyItem, part: 'title' | 'detail' | 'control') => tDyn(`privacy.item.${item.id}.${part}`);

  /** "Always" is for local reads that no switch can stop. */
  function status(item: PrivacyItem): 'on' | 'off' | 'manual' | 'always' {
    const active = privacyItemActive(item, s);
    if (active === null) return item.kind === 'local' ? 'always' : 'manual';
    return active ? 'on' : 'off';
  }

  const GROUPS = [
    { kind: 'local', title: 'privacy.local' },
    { kind: 'request', title: 'privacy.requests' },
  ] as const;

  const searchText = $derived(
    [t('privacy.intro'), t('privacy.local'), t('privacy.requests'), ...PRIVACY_ITEMS.flatMap((i) => [text(i, 'title'), text(i, 'detail'), text(i, 'control')])].join(' ')
  );
</script>

<SettingsCard id="privacy" title={t('settings.card.privacy')} reset="privacy" wide>
  <Field label={t('settings.hideAccountEmail')} hint={t('settings.hideAccountEmail.hint')}>
    <Toggle
      checked={s.hideAccountEmail}
      label={t('settings.hideAccountEmail')}
      onchange={(v) => void settings.patch({ hideAccountEmail: v })}
    />
  </Field>

  <SettingsBlock text={searchText}>
    <p class="intro">{t('privacy.intro')}</p>
    {#each GROUPS as group (group.kind)}
      <h4>{tDyn(group.title)}</h4>
      <ul>
        {#each PRIVACY_ITEMS.filter((i) => i.kind === group.kind) as item (item.id)}
          {@const st = status(item)}
          <li>
            <div class="row">
              <span class="title">{text(item, 'title')}</span>
              <span class="status {st}">{tDyn(`privacy.status.${st}`)}</span>
            </div>
            <p class="detail">{text(item, 'detail')}</p>
            <p class="control">{t('privacy.controlledBy', { control: text(item, 'control') })}</p>
          </li>
        {/each}
      </ul>
    {/each}
  </SettingsBlock>
</SettingsCard>

<style>
  .intro {
    margin: 0.5rem 0 0;
    font-size: 0.8125rem;
    color: var(--muted);
  }

  h4 {
    margin: 0.875rem 0 0.25rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
  }

  li:last-child {
    border-bottom: none;
  }

  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .title {
    font-weight: 500;
  }

  .status {
    flex: none;
    font-size: 0.6875rem;
    padding: 0.0625rem 0.4375rem;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    color: var(--muted);
    white-space: nowrap;
  }

  .status.on {
    color: var(--focus);
  }

  .detail,
  .control {
    margin: 0.1875rem 0 0;
    font-size: 0.75rem;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .control {
    color: var(--faint);
  }
</style>
