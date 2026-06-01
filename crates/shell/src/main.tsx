import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { getCurrentWebview } from '@tauri-apps/api/webview'

import App from '@/App'
import { OverlayRoot } from '@/overlay/OverlayRoot'

import '@/styles/tokens.css'
import '@/styles/window.css'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      staleTime: Infinity,
      retry: 0,
    },
    mutations: {
      retry: 0,
    },
  },
})

// The webview label is the source of truth for which surface to render:
// "main" → the full chrome (App); "overlay-<name>" → the surface for that
// overlay.
const label = getCurrentWebview().label
const OVERLAY_PREFIX = 'overlay-'
const surface = label.startsWith(OVERLAY_PREFIX) ? label.slice(OVERLAY_PREFIX.length) : null

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
