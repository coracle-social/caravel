import { Show } from "solid-js"
import type { Tab } from "@/shared/lib/loginInput"

// Presentational tab-selection panel for the login screen. All login logic stays
// in Login.tsx; this only renders tabs and surfaces continue callbacks. Props are
// reactive only when read lazily, so access props.* inside JSX, never destructure
// signal-bearing props at the top.
type LoginTabsScreenProps = {
  tab: () => Tab
  setTab: (tab: Tab) => void
  loading: () => boolean
  hasExtension: boolean
  onContinueExtension: () => void
  onContinueSigner: () => void
  onContinueKey: () => void
}

export default function LoginTabsScreen(props: LoginTabsScreenProps) {
  return (
    <>
      <h2 class="text-lg font-semibold text-gray-900">Log in / Sign up</h2>
      <p class="mt-2 text-xs text-gray-500">
        Use any Nostr signer method. New users are automatically onboarded.
      </p>
      <div class="mt-6 space-y-4">
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.tab() === "nip07" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setTab("nip07")}
            disabled={!props.hasExtension}
          >
            Extension
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.tab() === "nip46" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setTab("nip46")}
          >
            Signer
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.tab() === "key" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setTab("key")}
          >
            Key
          </button>
        </div>

        <Show when={props.tab() === "nip07"}>
          <button
            type="button"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
            disabled={!props.hasExtension || props.loading()}
            onClick={props.onContinueExtension}
          >
            {props.loading() ? "Connecting..." : <>Continue with extension <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg></>}
          </button>
        </Show>

        <Show when={props.tab() === "nip46"}>
          <button
            type="button"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium"
            onClick={props.onContinueSigner}
          >
            Continue with signer <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          </button>
        </Show>

        <Show when={props.tab() === "key"}>
          <button
            type="button"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium"
            onClick={props.onContinueKey}
          >
            Continue with key <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          </button>
        </Show>
      </div>
    </>
  )
}
