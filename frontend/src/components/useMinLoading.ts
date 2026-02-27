import { createEffect, createSignal, onCleanup } from "solid-js"

export default function useMinLoading(loading: () => boolean, minDurationMs = 200) {
  const [visible, setVisible] = createSignal(false)
  let startTime = 0
  let timeout: number | undefined

  createEffect(() => {
    if (timeout) {
      window.clearTimeout(timeout)
      timeout = undefined
    }

    if (loading()) {
      startTime = Date.now()
      setVisible(true)
      return
    }

    if (!visible()) return

    const elapsed = Date.now() - startTime
    const remaining = Math.max(minDurationMs - elapsed, 0)

    if (remaining === 0) {
      setVisible(false)
      return
    }

    timeout = window.setTimeout(() => {
      setVisible(false)
      timeout = undefined
    }, remaining)

    onCleanup(() => {
      if (timeout) window.clearTimeout(timeout)
    })
  })

  return visible
}
