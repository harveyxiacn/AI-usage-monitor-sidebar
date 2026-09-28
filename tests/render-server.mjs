import { createServer } from 'vite';
import path from 'node:path';

const port = Number(process.env.RENDER_PORT ?? 14897);
const server = await createServer({ tsconfig: path.resolve('tsconfig.json'), server: { host: '127.0.0.1', port, strictPort: true, fs: { allow: [path.resolve('.')] } },
  plugins: [{ name: 'read-only-render-test-server', configureServer(devServer) {
    // Test-only loopback shutdown avoids a Windows shell/tree-kill teardown hang.
    devServer.middlewares.use('/__render_shutdown', (_request, response) => {
      response.end('closing');
      setTimeout(() => void devServer.close().then(() => process.exit(0)), 25);
    });
  } }],
});
await server.listen();
