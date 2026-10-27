import { createSignal, onCleanup, Show } from "solid-js"
import QRCode from "qrcode"
import { ExtensionSigner, NostrConnectSigner, type ISigner } from "applesauce-signers"
import { formatError } from "@/shared/lib/format"
import { createSession } from "@/shared/lib/http"
import { NOSTR_CONNECT_RELAYS } from "@/portal/lib/nostr"
import { signIn } from "@/portal/lib/session"

const PERMISSIONS = NostrConnectSigner.buildSigningPermissions([27235])

// Signing in by signing one request with an extension or a remote signer, keeping only the session it buys.
export default function SignIn() {
  const [remote, setRemote] = createSignal(false)
  const [uri, setUri] = createSignal("")
  const [qr, setQr] = createSignal("")
  const [bunker, setBunker] = createSignal("")
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  // Aborting it stops waiting for the signer app, which closes its signer.
  let abort: AbortController | undefined

  const stopRemote = () => abort?.abort()

  onCleanup(stopRemote)

  // Sign in with a signer, and say whether it worked.
  async function complete(using: ISigner) {
    setBusy(true)
    setError("")
    try {
      signIn((await createSession(using)).token)
      return true
    } catch (e) {
      setError(formatError(e, "Signing in failed"))
      return false
    } finally {
      setBusy(false)
    }
  }

  // Offer signer-app codes until one signs in or the flow stops, with a new one after a failed sign-in.
  async function startRemote() {
    stopRemote()
    const stop = (abort = new AbortController())
    setRemote(true)
    setError("")

    while (!stop.signal.aborted) {
      setQr("")
      const signer = new NostrConnectSigner({ relays: NOSTR_CONNECT_RELAYS })
      const link = signer.getNostrConnectURI({ name: "Membership", url: location.origin, permissions: PERMISSIONS })
      setUri(link)
      const image = await QRCode.toDataURL(link, { width: 240, margin: 1 })
      if (stop.signal.aborted) return
      setQr(image)

      try {
        await signer.waitForSigner(stop.signal)
        if (await complete(signer)) return
      } catch (e) {
        if (!stop.signal.aborted) setError(formatError(e, "Your signer didn't connect"))
        return
      } finally {
        void signer.close()
      }
    }
  }

  async function connectBunker(e: Event) {
    e.preventDefault()
    setBusy(true)
    setError("")
    try {
      const signer = await NostrConnectSigner.fromBunkerURI(bunker().trim(), { permissions: PERMISSIONS })
      await complete(signer)
      void signer.close()
    } catch (e) {
      setError(formatError(e, "That bunker link didn't work"))
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
            <button
              type="button"
              onClick={() => { stopRemote(); setRemote(false) }}
              disabled={busy()}
              class="w-full text-sm text-zinc-500 hover:text-zinc-700 disabled:opacity-50"
            >
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
