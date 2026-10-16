import { createEffect, createSignal, onCleanup, Show, type JSX } from "solid-js"

const DURATION_MS = 200

// A panel over the page, which fades in and out. By default it is a centered dialog.
export default function Modal(props: {
  open: boolean
  onClose: () => void
  children: JSX.Element
  wrapperClass?: string
  panelClass?: string
}) {
  const [visible, setVisible] = createSignal(false)
  const [entered, setEntered] = createSignal(false)
  let timer: number | undefined

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
    }, DURATION_MS)
  })

  onCleanup(() => {
    if (timer) window.clearTimeout(timer)
  })

  const wrapperClass = () => props.wrapperClass ?? "fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
  const panelClass = () => props.panelClass ?? "w-full max-w-md rounded-2xl bg-white shadow-xl overflow-hidden"

  return (
    <Show when={visible()}>
      <div
        class={`transition-opacity duration-200 ${entered() ? "opacity-100" : "opacity-0"} ${wrapperClass()}`}
        role="dialog"
        aria-modal="true"
        onClick={props.onClose}
      >
        <div
          class={`transition-all duration-200 ${entered() ? "translate-y-0 opacity-100" : "translate-y-16 opacity-0"} ${panelClass()}`}
          onClick={(e) => e.stopPropagation()}
        >
          {props.children}
        </div>
      </div>
    </Show>
  )
}
