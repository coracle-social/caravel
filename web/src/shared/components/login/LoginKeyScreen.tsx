import { Show } from "solid-js"
import SegmentedTabs from "@/shared/components/SegmentedTabs"

type KeyTab = "plaintext" | "encrypted"

type LoginKeyScreenProps = {
  keyTab: () => KeyTab
  setKeyTab: (tab: KeyTab) => void
  nsecValue: () => string
  setNsecValue: (value: string) => void
  ncryptsecValue: () => string
  setNcryptsecValue: (value: string) => void
  password: () => string
  setPassword: (value: string) => void
  loading: () => boolean
  onBack: () => void
  onSubmit: () => void
}

// Logging in with a pasted nsec, or an ncryptsec and its password.
export default function LoginKeyScreen(props: LoginKeyScreenProps) {
  return (
    <>
      <button
        type="button"
        class="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-900"
        onClick={props.onBack}
      >
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
        Back
      </button>
      <h2 class="mt-3 text-lg font-semibold text-gray-900">Log in with key</h2>
      <div class="mt-4 space-y-4">
        <SegmentedTabs
          tabs={[{ value: "plaintext", label: "Plaintext" }, { value: "encrypted", label: "Encrypted" }]}
          selected={props.keyTab()}
          onSelect={props.setKeyTab}
        />

        <form
          class="space-y-3"
          onSubmit={(e) => {
            e.preventDefault()
            props.onSubmit()
          }}
        >
          <Show when={props.keyTab() === "plaintext"}>
            <input
              value={props.nsecValue()}
              onInput={(e) => props.setNsecValue(e.currentTarget.value)}
              placeholder="nsec1..."
              class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
            />
          </Show>
          <Show when={props.keyTab() === "encrypted"}>
            <input
              value={props.ncryptsecValue()}
              onInput={(e) => props.setNcryptsecValue(e.currentTarget.value)}
              placeholder="ncryptsec1..."
              class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
            />
            <input
              type="password"
              value={props.password()}
              onInput={(e) => props.setPassword(e.currentTarget.value)}
              placeholder="Password"
              class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
            />
          </Show>
          <button
            type="submit"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
            disabled={props.loading()}
          >
            Log in
          </button>
        </form>
      </div>
    </>
  )
}
