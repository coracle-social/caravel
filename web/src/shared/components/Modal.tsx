import { createEffect, createSignal, onCleanup, Show, type JSX } from "solid-js"

type ModalProps = {
  open: boolean
  onClose: () => void
  children: JSX.Element
  wrapperClass?: string
  panelClass?: string
  durationMs?: number
}

export default function Modal(props: ModalProps) {
  const [visible, setVisible] = createSignal(false)
  const [entered, setEntered] = createSignal(false)
  let timer: number | undefined

  const duration = () => props.durationMs ?? 200

  createEffect(() => {
    if (timer) window.clearTimeout(timer)

    if (props.open) {
      setVisible(true)
      setEntered(false)
      timer = window.setTimeout(() => setEntered(true), 10)
      return
    }

    if (!visible()) return

    setEntered(false)
    timer = window.setTimeout(() => {
      setVisible(false)
    }, duration())
  })

  onCleanup(() => {
    if (timer) window.clearTimeout(timer)
  })

  return (
    <Show when={visible()}>
      <div
        class={`transition-opacity duration-200 ${entered() ? "opacity-100" : "opacity-0"} ${props.wrapperClass ?? ""}`.trim()}
        role="dialog"
        aria-modal="true"
        onClick={props.onClose}
      >
        <div
          class={`transition-all duration-200 ${entered() ? "translate-y-0 opacity-100" : "translate-y-16 opacity-0"} ${props.panelClass ?? ""}`.trim()}
          onClick={(e) => e.stopPropagation()}
        >
          {props.children}
        </div>
      </div>
    </Show>
  )
}
