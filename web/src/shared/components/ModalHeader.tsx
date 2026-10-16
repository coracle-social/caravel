import type { JSX } from "solid-js"

// A dialog's title, with what belongs under it, and a close button.
export default function ModalHeader(props: { title: string; onClose: () => void; closeDisabled?: boolean; children?: JSX.Element }) {
  return (
    <div class="px-6 pt-6 pb-4 border-b border-gray-100">
      <div class="flex items-start justify-between gap-3">
        <div>
          <h2 class="text-lg font-semibold text-gray-900">{props.title}</h2>
          {props.children}
        </div>
        <button
          type="button"
          onClick={props.onClose}
          disabled={props.closeDisabled}
          class="shrink-0 rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 disabled:cursor-not-allowed disabled:opacity-50"
          aria-label="Close"
        >
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18 6L6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
    </div>
  )
}
