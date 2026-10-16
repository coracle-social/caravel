import { Show } from "solid-js"
import SegmentedTabs from "@/shared/components/SegmentedTabs"
import type { Tab } from "@/shared/lib/loginInput"

type LoginTabsScreenProps = {
  tab: () => Tab
  setTab: (tab: Tab) => void
  loading: () => boolean
  hasExtension: boolean
  onContinueExtension: () => void
  onContinueSigner: () => void
  onContinueKey: () => void
}

// Choosing how to log in: a browser extension, a remote signer, or a key.
export default function LoginTabsScreen(props: LoginTabsScreenProps) {
  return (
    <>
      <h2 class="text-lg font-semibold text-gray-900">Log in / Sign up</h2>
      <p class="mt-2 text-xs text-gray-500">
        Use any Nostr signer method. New users are automatically onboarded.
      </p>
      <div class="mt-6 space-y-4">
        <SegmentedTabs
          tabs={[
            { value: "nip07", label: "Extension", disabled: !props.hasExtension },
            { value: "nip46", label: "Signer" },
            { value: "key", label: "Key" },
          ]}
          selected={props.tab()}
          onSelect={props.setTab}
        />

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
