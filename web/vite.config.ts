import { defineConfig, loadEnv, type Plugin } from 'vite'
import solid from 'vite-plugin-solid'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

const page = (name: string) => fileURLToPath(new URL(`./${name}.html`, import.meta.url))

// Under `vite dev`, a page loads the app the server would serve for it: the
// admin dashboard under /admin, the member portal under /portal, and the tenant
// dashboard everywhere else.
const pickApp = (): Plugin => ({
  name: 'pick-app',
  configureServer(server) {
    server.middlewares.use((req, _res, next) => {
      const path = (req.url ?? '/').split('?')[0]
      if (req.method === 'GET' && req.headers.accept?.includes('text/html')) {
        const query = req.url?.includes('?') ? req.url.slice(req.url.indexOf('?')) : ''
        if (path.startsWith('/portal')) req.url = `/portal.html${query}`
        else if (path === '/admin' || path.startsWith('/admin/')) req.url = `/admin.html${query}`
        else req.url = `/tenant.html${query}`
      }
      next()
    })
  },
})

export default defineConfig(({mode}) => {
  const env = loadEnv(mode, process.cwd(), '')

  return {
    plugins: [pickApp(), tailwindcss(), solid()],
    resolve: {
      alias: {
        '@': fileURLToPath(new URL('./src', import.meta.url)),
      },
    },
    build: {
      rollupOptions: {
        input: {
          tenant: page('tenant'),
          admin: page('admin'),
          portal: page('portal'),
        },
      },
    },
    server: {
      port: Number(env.VITE_PORT) || 5173,
    },
  }
})
