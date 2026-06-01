import { For, Show } from "solid-js"

export type PromptBannerAction = {
  label: string
  onClick: () => void
}

type Severity = "error" | "warn" | "info"

type PromptBannerProps = {
  severity: Severity
  message: string
  actions?: PromptBannerAction[]
  onDismiss?: () => void
  class?: string
}

const containerStyles: Record<Severity, string> = {
  error: "border-red-200 bg-red-50 text-red-800",
  warn: "border-amber-200 bg-amber-50 text-amber-800",
  info: "border-blue-200 bg-blue-50 text-blue-800",
}

const actionStyles: Record<Severity, string> = {
  error: "text-red-800 hover:text-red-900",
  warn: "text-amber-800 hover:text-amber-900",
  info: "text-blue-800 hover:text-blue-900",
}

export default function PromptBanner(props: PromptBannerProps) {
  return (
    <div class={`rounded-lg border p-4 flex items-start justify-between gap-4 ${containerStyles[props.severity]} ${props.class ?? ""}`.trim()}>
      <p class="text-sm min-w-0">{props.message}</p>
      <div class="flex items-center gap-3 shrink-0">
        <For each={props.actions ?? []}>
          {(action) => (
            <button
              type="button"
              onClick={action.onClick}
              class={`text-sm font-medium underline hover:no-underline whitespace-nowrap ${actionStyles[props.severity]}`}
            >
              {action.label}
            </button>
          )}
        </For>
        <Show when={props.onDismiss}>
          <button
            type="button"
            onClick={() => props.onDismiss?.()}
            aria-label="Dismiss"
            class={`shrink-0 opacity-70 hover:opacity-100 ${actionStyles[props.severity]}`}
          >
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </Show>
      </div>
    </div>
  )
}
