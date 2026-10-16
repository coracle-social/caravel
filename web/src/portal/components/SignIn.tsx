import { createSignal, onCleanup, Show } from "solid-js"
import QRCode from "qrcode"
import { ExtensionSigner, NostrConnectSigner, type ISigner } from "applesauce-signers"
import { NOSTR_CONNECT_RELAYS, signInWith } from "@/portal/lib/nostr"
import { signIn } from "@/portal/lib/session"

const PERMISSIONS = NostrConnectSigner.buildSigningPermissions([27235])

// Sign in with Nostr by signing one request, from a browser extension or a
// remote signer. Nothing is kept but the token it buys.
export default function SignIn() {
  const [remote, setRemote] = createSignal(false)
  const [uri, setUri] = createSignal("")
  const [qr, setQr] = createSignal("")
  const [bunker, setBunker] = createSignal("")
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  let abort: AbortController | undefined
  let signer: NostrConnectSigner | undefined

  onCleanup(() => {
    abort?.abort()
    void signer?.close()
  })

  async function complete(using: ISigner) {
    setBusy(true)
    setError("")
    try {
      signIn((await signInWith(using)).token)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Signing in failed")
    } finally {
      setBusy(false)
    }
  }

  async function startRemote() {
    setRemote(true)
    setError("")

    signer = new NostrConnectSigner({ relays: NOSTR_CONNECT_RELAYS })
    const link = signer.getNostrConnectURI({ name: "Membership", url: location.origin, permissions: PERMISSIONS })
    setUri(link)
    setQr(await QRCode.toDataURL(link, { width: 240, margin: 1 }))

    abort = new AbortController()
    try {
      await signer.waitForSigner(abort.signal)
      await complete(signer)
    } catch (e) {
      if (!abort.signal.aborted) setError(e instanceof Error ? e.message : "Your signer didn't connect")
    }
  }

  async function connectBunker(e: Event) {
    e.preventDefault()
    setBusy(true)
    setError("")
    try {
      signer = await NostrConnectSigner.fromBunkerURI(bunker().trim(), { permissions: PERMISSIONS })
      await complete(signer)
    } catch (e) {
      setError(e instanceof Error ? e.message : "That bunker link didn't work")
      setBusy(false)
    }
  }

  const button = "w-full rounded-xl px-4 py-3 text-sm font-medium transition-colors disabled:opacity-50"

  return (
    <div class="space-y-3">
      <Show
        when={!remote()}
        fallback={
          <div class="space-y-4">
            <p class="text-sm text-zinc-600">Scan this with your signer app, or open the link on this device.</p>
            <Show when={qr()}>
              <img src={qr()} alt="Sign-in QR code" class="mx-auto h-60 w-60 rounded-lg border border-zinc-200" />
            </Show>
            <a href={uri()} class={`${button} block bg-zinc-100 text-center text-zinc-800 hover:bg-zinc-200`}>
              Open in signer app
            </a>
            <form onSubmit={connectBunker} class="flex gap-2">
              <input
                value={bunker()}
                onInput={(e) => setBunker(e.currentTarget.value)}
                placeholder="bunker://…"
                autocomplete="off"
                autocapitalize="none"
                spellcheck={false}
                class="min-w-0 flex-1 rounded-xl border border-zinc-300 px-3 py-2 font-mono text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
              />
              <button
                type="submit"
                disabled={!bunker().trim() || busy()}
                class="rounded-xl bg-zinc-900 px-4 py-2 text-sm font-medium text-white hover:bg-zinc-700 disabled:opacity-50"
              >
                Connect
              </button>
            </form>
            <button type="button" onClick={() => setRemote(false)} class="w-full text-sm text-zinc-500 hover:text-zinc-700">
              Back
            </button>
          </div>
        }
      >
        <Show when={window.nostr}>
          <button
            type="button"
            onClick={() => void complete(new ExtensionSigner())}
            disabled={busy()}
            class={`${button} bg-indigo-600 text-white hover:bg-indigo-500`}
          >
            {busy() ? "Signing in…" : "Sign in with browser extension"}
          </button>
        </Show>
        <button
          type="button"
          onClick={() => void startRemote()}
          disabled={busy()}
          class={window.nostr
            ? `${button} border border-zinc-300 text-zinc-800 hover:bg-zinc-50`
            : `${button} bg-indigo-600 text-white hover:bg-indigo-500`}
        >
          Sign in with a signer app
        </button>
      </Show>
      <Show when={error()}>
        <p class="text-sm text-red-600">{error()}</p>
      </Show>
    </div>
  )
}
