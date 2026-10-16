import { Show, createEffect, createSignal } from "solid-js"
import Modal from "@/shared/components/Modal"
import type { Relay } from "@/shared/lib/api"

type Props = {
  open: boolean
  onClose: () => void
  relay: () => Relay | undefined
  saving: () => boolean
  error: () => string | undefined
  onSave: (domain: string) => Promise<void>
}

export default function CustomDomainModal(props: Props) {
  const [input, setInput] = createSignal("")

  createEffect(() => {
    if (props.open) {
      setInput(props.relay()?.custom_domain ?? "")
    }
  })

  const current = () => props.relay()?.custom_domain ?? ""
  const inputTrimmed = () => input().trim()

  async function handleSubmit(e: Event) {
    e.preventDefault()
    await props.onSave(inputTrimmed())
    if (!props.error()) props.onClose()
  }

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      panelClass="w-full max-w-lg bg-white rounded-2xl shadow-xl p-6 space-y-5"
    >
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-semibold text-gray-900">Custom domain</h2>
        <button
          type="button"
          onClick={props.onClose}
          class="text-gray-400 hover:text-gray-600 text-xl leading-none"
          aria-label="Close"
        >
          ×
        </button>
      </div>

      <form onSubmit={handleSubmit} class="space-y-3">
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Custom domain</label>
          <input
            type="text"
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            placeholder="relay.example.com"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
            autocomplete="off"
            autocapitalize="none"
            spellcheck={false}
          />
          <p class="mt-1 text-xs text-gray-500">
            Must be a domain you control, e.g. <span class="font-mono">relay.example.com</span>
          </p>
        </div>
        <Show when={props.error()}>
          <p class="text-sm text-red-600">{props.error()}</p>
        </Show>
        <div class="flex items-center gap-3 flex-wrap">
          <button
            type="submit"
            disabled={props.saving() || !inputTrimmed() || inputTrimmed() === current()}
            class="inline-flex items-center rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {props.saving() ? "Saving…" : "Save domain"}
          </button>
          <Show when={current()}>
            <button
              type="button"
              onClick={() => props.onSave("")}
              disabled={props.saving()}
              class="inline-flex items-center rounded-lg border border-red-200 px-3 py-2 text-sm font-medium text-red-600 hover:bg-red-50 disabled:opacity-50"
            >
              {props.saving() ? "Removing…" : "Remove custom domain"}
            </button>
          </Show>
        </div>
      </form>
    </Modal>
  )
}
