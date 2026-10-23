import { createEffect, createSignal, on, Show } from "solid-js"
import { normalizeToPubkey } from "applesauce-core/helpers/pointers"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { formatError } from "@/shared/lib/format"

type PubkeyDialogProps = {
  open: boolean
  onClose: () => void
  title: string
  description?: string
  submitLabel: string
  // A second, optional field after the pubkey, such as why someone is banned.
  noteLabel?: string
  onSubmit: (pubkey: string, note: string) => Promise<void>
}

// Asks for a pubkey, as an npub, nprofile or hex, and does something with it.
export default function PubkeyDialog(props: PubkeyDialogProps) {
  const [input, setInput] = createSignal("")
  const [note, setNote] = createSignal("")
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  createEffect(on(() => props.open, (open) => {
    if (!open) return
    setInput("")
    setNote("")
    setError("")
  }))

  async function submit(e: Event) {
    e.preventDefault()
    const pubkey = normalizeToPubkey(input().trim())
    if (!pubkey) {
      setError("Enter an npub or a hex pubkey.")
      return
    }

    setBusy(true)
    setError("")
    try {
      await props.onSubmit(pubkey, note().trim())
      props.onClose()
    } catch (e) {
      setError(formatError(e, "Something went wrong"))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal open={props.open} onClose={() => !busy() && props.onClose()}>
      <ModalHeader title={props.title} onClose={props.onClose} closeDisabled={busy()}>
        <Show when={props.description}>
          <p class="mt-1 text-sm text-gray-500">{props.description}</p>
        </Show>
      </ModalHeader>
      <form onSubmit={submit} class="space-y-4 px-6 py-4">
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Pubkey</span>
          <input
            type="text"
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            placeholder="npub1..."
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
            autofocus
          />
        </label>
        <Show when={props.noteLabel}>
          <label class="block">
            <span class="mb-1 block text-sm font-medium text-gray-700">{props.noteLabel}</span>
            <input
              type="text"
              value={note()}
              onInput={(e) => setNote(e.currentTarget.value)}
              class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
            />
          </label>
        </Show>
        <Show when={error()}>
          <p class="text-sm text-red-600">{error()}</p>
        </Show>
        <div class="flex justify-end gap-2">
          <button type="button" onClick={props.onClose} disabled={busy()} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
            Cancel
          </button>
          <button type="submit" disabled={busy()} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
            {busy() ? "Saving..." : props.submitLabel}
          </button>
        </div>
      </form>
    </Modal>
  )
}
