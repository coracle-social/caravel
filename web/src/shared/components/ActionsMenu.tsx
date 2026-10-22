import { createEffect, createSignal, onCleanup, type JSX } from "solid-js"
import menuDotsIcon from "@/shared/assets/menu-dots-2.svg"

// A dots button, or `trigger` with its `triggerClass`, opening a menu of
// ActionsMenuItems, which closes on a click outside, Escape, or the `close` its
// children are given.
export default function ActionsMenu(props: {
  children: (close: () => void) => JSX.Element
  trigger?: JSX.Element
  triggerClass?: string
  label?: string
}) {
  const [open, setOpen] = createSignal(false)

  let containerRef: HTMLDivElement | undefined

  createEffect(() => {
    if (!open()) return

    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node | null
      if (target && !containerRef?.contains(target)) {
        setOpen(false)
      }
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false)
      }
    }

    document.addEventListener("mousedown", handleClickOutside)
    document.addEventListener("keydown", handleEscape)

    onCleanup(() => {
      document.removeEventListener("mousedown", handleClickOutside)
      document.removeEventListener("keydown", handleEscape)
    })
  })

  return (
    <div class="relative shrink-0" ref={containerRef}>
      <button
        type="button"
        class={props.triggerClass ?? "inline-flex h-9 w-9 items-center justify-center rounded-lg border border-gray-200 hover:bg-gray-50"}
        aria-label={props.label ?? "Open actions"}
        aria-expanded={open()}
        onClick={() => setOpen((isOpen) => !isOpen)}
      >
        {props.trigger ?? <img src={menuDotsIcon} alt="" class="h-5 w-5" />}
      </button>

      <div
        class="absolute right-0 mt-2 w-44 rounded-lg border border-gray-200 bg-white shadow-lg z-10 py-1 origin-top-right transition-all duration-150 ease-out"
        classList={{
          "opacity-100 scale-100": open(),
          "opacity-0 scale-95 pointer-events-none": !open(),
        }}
      >
        {props.children(() => setOpen(false))}
      </div>
    </div>
  )
}

const ITEM_TONE_STYLES = {
  default: "text-gray-700 hover:bg-gray-50",
  primary: "text-blue-600 hover:bg-blue-50",
  danger: "text-red-600 hover:bg-red-50",
}

export function ActionsMenuItem(props: {
  tone?: keyof typeof ITEM_TONE_STYLES
  disabled?: boolean
  onClick: () => void
  children: JSX.Element
}) {
  return (
    <button
      type="button"
      class={`block w-full text-left px-3 py-2 text-sm disabled:opacity-50 ${ITEM_TONE_STYLES[props.tone ?? "default"]}`}
      onClick={props.onClick}
      disabled={props.disabled}
    >
      {props.children}
    </button>
  )
}
