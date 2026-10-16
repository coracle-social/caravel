import { copyToClipboard } from "@/shared/lib/clipboard"
import { shortenNpub, toNpub } from "@/shared/lib/pubkey"

// A pubkey as a short npub with a button copying the whole one, which works
// inside a link without following it. `class` styles the npub.
export default function CopyNpub(props: { pubkey: string; class?: string }) {
  const copy = (event: MouseEvent) => {
    event.preventDefault()
    event.stopPropagation()
    void copyToClipboard(toNpub(props.pubkey), { successMessage: "npub copied to clipboard" })
  }

  return (
    <span class="inline-flex items-center gap-1 min-w-0">
      <span class={`truncate ${props.class ?? ""}`}>{shortenNpub(props.pubkey)}</span>
      <button
        type="button"
        onClick={copy}
        title="Copy npub"
        aria-label="Copy npub"
        class="shrink-0 opacity-60 transition-opacity hover:opacity-100"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="9" y="9" width="13" height="13" rx="2" />
          <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
        </svg>
      </button>
    </span>
  )
}
