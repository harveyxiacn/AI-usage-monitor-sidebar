<!--
  "What's new": after the app version changed (`lastSeenVersion`), a dismissible
  panel with the bundled release notes of the running version, in the current
  language. `open` forces it open (for an About link); dismissing records the
  version so it never returns for it. [FRONTEND]
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import MarkdownView from '$lib/components/MarkdownView.svelte';
  import { getAppInfo } from '$lib/api';
  import { getLocale, t } from '$lib/i18n/i18n.svelte';
  import { hasReleaseNotes, loadReleaseNotes } from '$lib/release-notes';
  import { settings } from '$lib/stores/settings.svelte';
  import { notesFor, shouldShowWhatsNew, type ReleaseNotes } from '$lib/whats-new';

  interface Props {
    /** show regardless of `lastSeenVersion` (a "What's new" link in About) */
    forced?: boolean;
    ondismiss?: () => void;
  }

  let { forced = false, ondismiss }: Props = $props();

  let version = $state('');
  let notes = $state<ReleaseNotes | null>(null);

  onMount(() => {
    let alive = true;
    void getAppInfo()
      .then((info) => {
        if (alive) version = info.version;
      })
      .catch(() => {});
    return () => (alive = false);
  });

  const s = $derived(settings.value);
  const hasNotes = $derived(!!version && hasReleaseNotes(version));
  const visible = $derived(
    settings.loaded &&
      hasNotes &&
      (forced || shouldShowWhatsNew({ onboarded: s.onboarded, lastSeenVersion: s.lastSeenVersion, currentVersion: version, hasNotes }))
  );

  $effect(() => {
    if (visible && !notes && version) {
      void loadReleaseNotes(version)
        .then((loaded) => (notes = loaded))
        .catch(() => {
          /* the panel is a courtesy: a notes file that fails to load simply does not show */
        });

    }
  });

  // A version with no bundled notes (a dev build) is recorded quietly.
  $effect(() => {
    if (settings.loaded && s.onboarded && version && !hasNotes && s.lastSeenVersion !== version) {
      void settings.patch({ lastSeenVersion: version });
    }
  });

  function dismiss() {
    if (version && s.lastSeenVersion !== version) void settings.patch({ lastSeenVersion: version });
    ondismiss?.();
  }

  const body = $derived(notes ? notesFor(notes, getLocale()) : '');
</script>

{#if visible && notes}
  <aside class="whatsnew card" aria-labelledby="whatsnew-title">
    <header>
      <h3 id="whatsnew-title">{t('whatsnew.title', { version })}</h3>
      <button class="btn" onclick={dismiss}>{t('whatsnew.dismiss')}</button>
    </header>
    <div class="scroll"><MarkdownView source={body} /></div>
  </aside>
{/if}

<style>
  .whatsnew {
    margin-bottom: 1rem;
    padding: 0.75rem 1rem;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h3 {
    margin: 0;
    font-size: 0.9375rem;
    font-weight: 600;
  }

  .scroll {
    max-height: 14rem;
    overflow-y: auto;
  }
</style>
