import { For, Show, createEffect, createSignal } from "solid-js"
import Modal from "@/components/Modal"

type ConfirmDialogProps = {
  open: boolean
  title: string
  description: string
  /** Optional bullet points shown in a warning box below the description */
  details?: string[]
  confirmLabel: string
  busyLabel?: string
  busy?: boolean
  tone?: "danger" | "primary"
  onConfirm: () => void | Promise<void>
  onClose: () => void
}

const TONE_STYLES: Record<NonNullable<ConfirmDialogProps["tone"]>, string> = {
  danger: "bg-red-600 text-white hover:bg-red-700",
  primary: "bg-blue-600 text-white hover:bg-blue-700",
}

const DETAIL_BOX_STYLES: Record<NonNullable<ConfirmDialogProps["tone"]>, string> = {
  danger: "border-amber-200 bg-amber-50 text-amber-800",
  primary: "border-blue-200 bg-blue-50 text-blue-800",
}

type ConfirmDialogSnapshot = {
  title: string
  description: string
  details?: string[]
  confirmLabel: string
  busyLabel?: string
  busy: boolean
  tone: NonNullable<ConfirmDialogProps["tone"]>
}

export default function ConfirmDialog(props: ConfirmDialogProps) {
  const [snapshot, setSnapshot] = createSignal<ConfirmDialogSnapshot>({
    title: props.title,
    description: props.description,
    details: props.details ? [...props.details] : undefined,
    confirmLabel: props.confirmLabel,
    busyLabel: props.busyLabel,
    busy: props.busy ?? false,
    tone: props.tone ?? "primary",
  })

  createEffect(() => {
    if (!props.open) return

    setSnapshot({
      title: props.title,
      description: props.description,
      details: props.details ? [...props.details] : undefined,
      confirmLabel: props.confirmLabel,
      busyLabel: props.busyLabel,
      busy: props.busy ?? false,
      tone: props.tone ?? "primary",
    })
  })

  const content = () => props.open
    ? {
        title: props.title,
        description: props.description,
        details: props.details,
        confirmLabel: props.confirmLabel,
        busyLabel: props.busyLabel,
        busy: props.busy ?? false,
        tone: props.tone ?? "primary",
      }
    : snapshot()
  const tone = () => content().tone
  const confirmText = () => content().busy ? (content().busyLabel ?? content().confirmLabel) : content().confirmLabel

  function handleClose() {
    if (props.busy) return
    props.onClose()
  }

  function handleConfirm() {
    if (props.busy) return
    void props.onConfirm()
  }

  return (
    <Modal
      open={props.open}
      onClose={handleClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      panelClass="w-full max-w-md rounded-2xl bg-white shadow-xl overflow-hidden"
    >
      <div class="px-6 pt-6 pb-4 border-b border-gray-100">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h2 class="text-lg font-semibold text-gray-900">{content().title}</h2>
          </div>
          <button
            type="button"
            onClick={handleClose}
            disabled={content().busy}
            class="shrink-0 rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 disabled:cursor-not-allowed disabled:opacity-50"
            aria-label="Close"
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      <div class="px-6 py-4">
        <div class="space-y-3 text-left">
          <p class="text-sm text-gray-600">{content().description}</p>
          <Show when={content().details && content().details!.length > 0}>
            <ul class={`w-full rounded-lg border px-4 py-3 space-y-1.5 ${DETAIL_BOX_STYLES[tone()]}`}>
              <For each={content().details}>
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
          disabled={content().busy}
          class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 transition-colors hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={handleConfirm}
          disabled={content().busy}
          class={`rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${TONE_STYLES[tone()]}`}
        >
          {confirmText()}
        </button>
      </div>
    </Modal>
  )
}