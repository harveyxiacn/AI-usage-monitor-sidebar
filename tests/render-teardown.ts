// Closes the render server once, after every *.render.ts file has run. A
// per-file `afterAll` would shut it down under the next file; the loopback
// endpoint avoids a Windows shell/tree-kill teardown hang (render-server.mjs).
export default async function globalTeardown() {
  await fetch(`http://127.0.0.1:${Number(process.env.RENDER_PORT ?? 14897)}/__render_shutdown`).catch(() => {});
}
