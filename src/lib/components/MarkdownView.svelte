<!--
  Renders the output of `parseMarkdown` with ordinary elements — never
  `{@html}` — so release notes cannot inject markup. Links go through the
  system browser. [FRONTEND]
-->
<script lang="ts">
  import { parseMarkdown, type Inline } from '$lib/markdown';
  import { openExternal } from '$lib/external';

  let { source }: { source: string } = $props();
  const blocks = $derived(parseMarkdown(source));

  function open(event: MouseEvent, href: string) {
    event.preventDefault();
    void openExternal(href);
  }
</script>

{#snippet inline(items: Inline[])}{#each items as item, i (i)}{#if item.type === 'strong'}<strong>{item.text}</strong>{:else if item.type === 'code'}<code>{item.text}</code>{:else if item.type === 'link'}<a href={item.href} rel="noopener noreferrer" onclick={(e) => open(e, item.href)}>{item.text}</a>{:else}{item.text}{/if}{/each}{/snippet}

<div class="md">
  {#each blocks as block, i (i)}
    {#if block.type === 'heading'}
      <h4 class={`h${block.level}`}>{@render inline(block.inlines)}</h4>
    {:else if block.type === 'list'}
      <ul>
        {#each block.items as item, j (j)}<li>{@render inline(item)}</li>{/each}
      </ul>
    {:else}
      <p>{@render inline(block.inlines)}</p>
    {/if}
  {/each}
</div>

<style>
  .md {
    font-size: 0.8125rem;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  p,
  ul {
    margin: 0.375rem 0;
  }

  ul {
    padding-left: 1.125rem;
  }

  li + li {
    margin-top: 0.25rem;
  }

  h4 {
    margin: 0.625rem 0 0.25rem;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  code {
    padding: 0 0.25rem;
    border-radius: 0.25rem;
    background: var(--surface-2);
    font-family: var(--font-mono);
    font-size: 0.75rem;
  }

  a {
    color: var(--focus);
    text-decoration: underline;
  }
</style>
