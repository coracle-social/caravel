import { Show } from "solid-js"

// Presentational scanner overlay chrome. The <video> ref is owned by the parent
// via the videoRef setter so the QrScanner instance and its lifecycle stay in
// Login.tsx (openScanner waits a microtask for this element to mount). Props are
// reactive only when read lazily, so access props.* inside JSX.
type QrScannerOverlayProps = {
  open: boolean
  onClose: () => void
  videoRef: (el: HTMLVideoElement) => void
}

export default function QrScannerOverlay(props: QrScannerOverlayProps) {
  return (
    <Show when={props.open}>
      <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onClick={props.onClose}>
        <div class="relative w-full max-w-sm rounded-2xl bg-white p-4 shadow-xl" onClick={(e) => e.stopPropagation()}>
          <div class="flex items-center justify-between mb-3">
            <h3 class="text-sm font-semibold text-gray-900">Scan QR Code</h3>
            <button type="button" class="text-gray-400 hover:text-gray-700" onClick={props.onClose}>
              <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>
            </button>
          </div>
          <video ref={props.videoRef} class="w-full rounded-lg" />
        </div>
      </div>
    </Show>
  )
}
