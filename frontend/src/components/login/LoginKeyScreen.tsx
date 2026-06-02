import { Show } from "solid-js"

type KeyTab = "plaintext" | "encrypted"

// Presentational key-login panel. The actual login (loginWithKeyMaterial) stays
// in Login.tsx and is invoked via onSubmit; preventDefault is handled here so the
// parent only supplies the async login. Props are reactive only when read lazily,
// so access props.* inside JSX, never destructure signal-bearing props at the top.
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
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.keyTab() === "plaintext" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setKeyTab("plaintext")}
          >
            Plaintext
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.keyTab() === "encrypted" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setKeyTab("encrypted")}
          >
            Encrypted
          </button>
        </div>

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
