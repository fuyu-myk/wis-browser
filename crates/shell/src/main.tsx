import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import App from '@/App'
import { OverlayRoot } from '@/overlay/OverlayRoot'
import { currentPlatform } from '@/lib/platform'

import '@/styles/fonts.css'
import '@/styles/tokens.css'
import '@/styles/window.css'

// Host OS for platform-conditional styling. Static for the session.
document.body.setAttribute('data-platform', currentPlatform())

// Queries and mutations are IPC to the backend, never the network.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      staleTime: Infinity,
      retry: 0,
      networkMode: 'always',
    },
    mutations: {
      retry: 0,
      networkMode: 'always',
    },
  },
})

// Which surface to render is carried in the URL query (`index.html?surface=<name>`,
// set by `ensure_overlay`), freeing overlay labels to be per-window (`overlay-<window>-<name>`)
// so each OS window has its own overlays. The chrome webview loads index.html with
// no query and renders the full App.
const surface = new URLSearchParams(window.location.search).get('surface')

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement)

if (surface) {
  document.body.classList.add('is-overlay-surface', `surface-${surface}`)
  root.render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <OverlayRoot surface={surface} />
      </QueryClientProvider>
    </React.StrictMode>,
  )
} else {
  root.render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </React.StrictMode>,
  )
}
