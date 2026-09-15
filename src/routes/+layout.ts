// SPA mode: no SSR, prerender an HTML shell per route so Tauri can open
// /, /popover and /dashboard directly from the static build.
export const ssr = false;
export const prerender = true;
