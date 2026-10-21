import { For, Show, createEffect, createSignal } from "solid-js"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"

export type Confirmation = {
  title: string
  description: string
  // Shown as bullet points in a box below the description.
  details?: string[]
  confirmLabel: string
  busyLabel: string
  tone: "danger" | "primary"
}

const TONE_STYLES: Record<Confirmation["tone"], string> = {
  danger: "bg-red-600 text-white hover:bg-red-700",
  primary: "bg-blue-600 text-white hover:bg-blue-700",
}

const DETAIL_BOX_STYLES: Record<Confirmation["tone"], string> = {
  danger: "border-amber-200 bg-amber-50 text-amber-800",
  primary: "border-blue-200 bg-blue-50 text-blue-800",
}

type Shown = { confirmation: Confirmation; busy: boolean }

// Asks to confirm `confirmation`, while there is one.
export default function ConfirmDialog(props: {
  confirmation?: Confirmation
  busy: boolean
  onConfirm: () => void | Promise<void>
  onClose: () => void
}) {
  // What was last asked, kept on screen while the dialog closes.
  const [last, setLast] = createSignal<Shown>()

  createEffect(() => {
    if (props.confirmation) setLast({ confirmation: props.confirmation, busy: props.busy })
  })

  const shown = () => (props.confirmation ? { confirmation: props.confirmation, busy: props.busy } : last())

  function handleClose() {
    if (props.busy) return
    props.onClose()
  }

  function handleConfirm() {
    if (props.busy) return
    void props.onConfirm()
  }

  // Solid compiles a prop of `a && b` to false, not undefined, when `a` is falsy.
  return (
    <Modal open={Boolean(props.confirmation)} onClose={handleClose}>
      <Show when={shown()}>
        {(s) => (
          <>
            <ModalHeader title={s().confirmation.title} onClose={handleClose} closeDisabled={s().busy} />

            <div class="px-6 py-4">
              <div class="space-y-3 text-left">
                <p class="text-sm text-gray-600">{s().confirmation.description}</p>
                <Show when={s().confirmation.details?.length}>
                  <ul class={`w-full rounded-lg border px-4 py-3 space-y-1.5 ${DETAIL_BOX_STYLES[s().confirmation.tone]}`}>
                    <For each={s().confirmation.details}>
                      {(item) => (
                        <li class="flex items-start gap-2 text-sm">
                          <span class="mt-0.5 shrink-0 select-none">•</span>
                          <span>{item}</span>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </div>
            </div>

            <div class="px-6 py-4 flex justify-end gap-3 border-t border-gray-100">
              <button
                type="button"
                onClick={handleClose}
                disabled={s().busy}
                class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 transition-colors hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleConfirm}
                disabled={s().busy}
                class={`rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${TONE_STYLES[s().confirmation.tone]}`}
              >
                {s().busy ? s().confirmation.busyLabel : s().confirmation.confirmLabel}
              </button>
            </div>
          </>
        )}
      </Show>
    </Modal>
  )
}
