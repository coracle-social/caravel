import { Show, createEffect, createSignal, onCleanup } from "solid-js"

type ToastProps = {
  message?: string
  duration?: number
  onClear?: () => void
}

export default function Toast(props: ToastProps) {
  const [visible, setVisible] = createSignal(false)

  let hideTimer: number | undefined
  let clearTimer: number | undefined
  let rafOne: number | undefined
  let rafTwo: number | undefined

  function clearTimers() {
    if (hideTimer) window.clearTimeout(hideTimer)
    if (clearTimer) window.clearTimeout(clearTimer)
    if (rafOne) window.cancelAnimationFrame(rafOne)
    if (rafTwo) window.cancelAnimationFrame(rafTwo)
    hideTimer = undefined
    clearTimer = undefined
    rafOne = undefined
    rafTwo = undefined
  }

  createEffect(() => {
    const message = props.message?.trim()
    clearTimers()

    if (!message) {
      setVisible(false)
      return
    }

    setVisible(false)
    rafOne = window.requestAnimationFrame(() => {
      rafTwo = window.requestAnimationFrame(() => {
        setVisible(true)
        rafOne = undefined
        rafTwo = undefined
      })
    })

    hideTimer = window.setTimeout(() => {
      setVisible(false)
      clearTimer = window.setTimeout(() => {
        props.onClear?.()
        clearTimer = undefined
      }, 250)
      hideTimer = undefined
    }, props.duration ?? 10_000)
  })

  onCleanup(() => {
    clearTimers()
  })

  return (
    <Show when={props.message}>
      <div
        role="alert"
        class="fixed bottom-4 right-4 z-50 max-w-md rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-base text-red-700 shadow-lg transition-all duration-400 ease-out"
        classList={{
          "translate-y-0 opacity-100 scale-100": visible(),
          "translate-y-3 opacity-0 scale-95": !visible(),
        }}
      >
        {props.message}
      </div>
    </Show>
  )
}
