// A deliberately tiny, *safe* markdown reader for the bundled release notes.
// [FRONTEND]
//
// It understands headings, bullet lists, paragraphs, **bold**, `code` and
// [links](https://…). Everything else stays literal text and nothing is ever
// turned into HTML: the result is a plain data tree that Svelte renders with
// ordinary elements, so a hostile note cannot inject markup. Links are kept
// only when they point at http(s); relative links (`../SESSIONS.md`) and
// anything else (`javascript:`) degrade to their label.

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'strong'; text: string }
  | { type: 'code'; text: string }
  | { type: 'link'; text: string; href: string };

export type Block =
  | { type: 'heading'; level: 1 | 2 | 3; inlines: Inline[] }
  | { type: 'paragraph'; inlines: Inline[] }
  | { type: 'list'; items: Inline[][] };

export function safeHref(href: string): string | null {
  try {
    const url = new URL(href.trim());
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
  } catch {
    return null;
  }
}

const INLINE = /\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)/g;

export function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  const push = (text: string) => {
    if (text) out.push({ type: 'text', text });
  };
  for (const match of source.matchAll(INLINE)) {
    push(source.slice(last, match.index));
    last = match.index + match[0].length;
    if (match[1] !== undefined) out.push({ type: 'strong', text: match[1] });
    else if (match[2] !== undefined) out.push({ type: 'code', text: match[2] });
    else {
      const href = safeHref(match[4]);
      if (href) out.push({ type: 'link', text: match[3], href });
      else push(match[3]);
    }
  }
  push(source.slice(last));
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: Inline[][] | null = null;
  const flush = () => {
    if (paragraph.length) blocks.push({ type: 'paragraph', inlines: parseInline(paragraph.join(' ')) });
    if (list) blocks.push({ type: 'list', items: list });
    paragraph = [];
    list = null;
  };
  for (const raw of source.replace(/\r\n?/g, '\n').split('\n')) {
    const line = raw.trimEnd();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const bullet = /^\s*[-*]\s+(.*)$/.exec(line);
    if (!line.trim()) {
      flush();
    } else if (heading) {
      flush();
      const level = Math.min(3, heading[1].length) as 1 | 2 | 3;
      blocks.push({ type: 'heading', level, inlines: parseInline(heading[2]) });
    } else if (bullet) {
      if (paragraph.length) flush();
      (list ??= []).push(parseInline(bullet[1]));
    } else {
      if (list) flush();
      paragraph.push(line.trim());
    }
  }
  flush();
  return blocks;
}
