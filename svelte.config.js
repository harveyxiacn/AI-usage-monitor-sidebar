// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import process from "node:process";
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  // Dev only: inject component CSS from JS. The separate `?svelte&type=style`
  // requests race with Vite's warm-up in the Tauri webview and can end up
  // serving the raw .svelte source as CSS; injected styles avoid that. The
  // production build keeps external CSS files.
  compilerOptions: {
    css: process.env.NODE_ENV === 'production' ? 'external' : 'injected',
  },
  kit: {
    adapter: adapter({
      // NOT "index.html": that name is taken by the prerendered "/" page (the
      // sidebar window loads build/index.html directly and needs the real
      // prerendered shell, not the catch-all SPA fallback).
      fallback: "200.html",
    }),
    // The three Tauri windows load build/index.html, build/popover.html and
    // build/dashboard.html directly. Nothing links between them, so the
    // prerender crawler cannot discover /popover and /dashboard on its own.
    prerender: {
      entries: ["/", "/popover", "/dashboard"],
    },
  },
};

export default config;
