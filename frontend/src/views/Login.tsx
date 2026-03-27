import { Show, createSignal, createEffect, onCleanup } from "solid-js"
import { useNavigate, type RouteSectionProps } from "@solidjs/router"
import { ExtensionAccount, NostrConnectAccount, PasswordAccount, PrivateKeyAccount } from "applesauce-accounts/accounts"
import { PasswordSigner } from "applesauce-signers"
import QrScanner from "qr-scanner"
import QRCode from "qrcode"
import { accountManager, identity, PLATFORM_NAME } from "@/lib/state"
import useMinLoading from "@/components/useMinLoading"

const NIP46_RELAYS = ['wss://bucket.coracle.social', 'wss://ephemeral.snowflare.cc']

type Tab = "nip07" | "nip46" | "key"
type Screen = "tabs" | "nip46" | "key"
type SignerTab = "qr" | "paste"
type KeyTab = "plaintext" | "encrypted"

type LoginProps = {
  inModal?: boolean
  onClose?: () => void
  onAuthenticated?: () => void | Promise<void>
}

function normalizeBunkerUrl(value: string): string {
  const trimmed = value.trim()
  if (!trimmed) return ""

  if (trimmed.startsWith("nostrconnect://")) {
    const url = new URL(trimmed)
    const remote = url.host || url.pathname.replace(/^\/+/, "")
    const relays = url.searchParams.getAll("relay")
    const secret = url.searchParams.get("secret")
    const params = new URLSearchParams()
    for (const relay of relays) params.append("relay", relay)
    if (secret) params.set("secret", secret)
    return `bunker://${remote}?${params.toString()}`
  }

  return trimmed
}

async function loadNostrConnectSigner() {
  return import("applesauce-signers").then((m) => m.NostrConnectSigner)
}

type LoginPageProps = LoginProps & Partial<RouteSectionProps<unknown>>

export default function Login(props: LoginPageProps = {}) {
  const navigate = useNavigate()
  const [tab, setTab] = createSignal<Tab>(window.nostr ? "nip07" : "nip46")
  const [rawLoading, setRawLoading] = createSignal(false)
  const loading = useMinLoading(() => rawLoading())
  const [error, setError] = createSignal("")
  const [screen, setScreen] = createSignal<Screen>("tabs")

  const [signerTab, setSignerTab] = createSignal<SignerTab>("qr")
  const [keyTab, setKeyTab] = createSignal<KeyTab>("plaintext")
  const [nostrConnectUri, setNostrConnectUri] = createSignal("")
  const [qrDataUrl, setQrDataUrl] = createSignal("")
  const [showScanner, setShowScanner] = createSignal(false)
  const [bunkerUrl, setBunkerUrl] = createSignal("")
  const [nsecValue, setNsecValue] = createSignal("")
  const [ncryptsecValue, setNcryptsecValue] = createSignal("")
  const [password, setPassword] = createSignal("")

  let scannerVideo: HTMLVideoElement | undefined
  let scanner: QrScanner | undefined
  let abortController: AbortController | undefined

  async function completeLogin(account: ExtensionAccount | NostrConnectAccount | PrivateKeyAccount | PasswordAccount) {
    accountManager.addAccount(account)
    accountManager.setActive(account)
    await props.onAuthenticated?.()
  }

  async function loginWithNip07() {
    setError("")
    setRawLoading(true)
    try {
      await completeLogin(await ExtensionAccount.fromExtension())
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to login with extension")
    } finally {
      setRawLoading(false)
    }
  }

  async function startNostrConnect() {
    setError("")
    setRawLoading(true)

    try {
      const NostrConnectSigner = await loadNostrConnectSigner()
      const signer = new NostrConnectSigner({ relays: NIP46_RELAYS })
      const uri = signer.getNostrConnectURI({
        name: PLATFORM_NAME,
        url: window.location.origin,
      })
      setNostrConnectUri(uri)
      setQrDataUrl(await QRCode.toDataURL(uri, { width: 256, margin: 2 }))

      abortController = new AbortController()
      const timeout = window.setTimeout(() => abortController?.abort(), 60_000)

      try {
        await signer.waitForSigner(abortController.signal)
        const pubkey = await signer.getPublicKey()
        const account = new NostrConnectAccount(pubkey, signer)
        await completeLogin(account)
      } finally {
        window.clearTimeout(timeout)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to connect signer")
    } finally {
      setRawLoading(false)
    }
  }

  async function loginWithBunker() {
    setError("")
    setRawLoading(true)
    try {
      const uri = normalizeBunkerUrl(bunkerUrl())
      const NostrConnectSigner = await loadNostrConnectSigner()
      const signer = await NostrConnectSigner.fromBunkerURI(uri)
      const pubkey = await signer.getPublicKey()
      const account = new NostrConnectAccount(pubkey, signer)
      await completeLogin(account)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Invalid bunker URL")
    } finally {
      setRawLoading(false)
    }
  }

  async function loginWithKeyMaterial() {
    setError("")
    setRawLoading(true)
    try {
      if (ncryptsecValue().trim()) {
        if (!password().trim()) {
          throw new Error("Password is required for ncryptsec")
        }
        const signer = await PasswordSigner.fromNcryptsec(ncryptsecValue().trim(), password())
        const pubkey = await signer.getPublicKey()
        const account = new PasswordAccount(pubkey, signer)
        await completeLogin(account)
        return
      }

      const key = nsecValue().trim()
      if (!key) throw new Error("Enter an nsec or ncryptsec key")
      const account = PrivateKeyAccount.fromKey(key)
      await completeLogin(account)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Invalid key")
    } finally {
      setRawLoading(false)
    }
  }

  function closeScanner() {
    setShowScanner(false)
    scanner?.destroy()
    scanner = undefined
  }

  async function openScanner() {
    setError("")
    setShowScanner(true)

    // Wait a tick for the modal video element to mount
    await new Promise(r => setTimeout(r, 0))
    if (!scannerVideo) return

    scanner = new QrScanner(
      scannerVideo,
      (result) => {
        setBunkerUrl(result.data)
        closeScanner()
      },
      { returnDetailedScanResult: true },
    )
    try {
      await scanner.start()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Unable to access camera")
      closeScanner()
    }
  }

  function enterSignerScreen() {
    setError("")
    setScreen("nip46")
    setSignerTab("qr")
    if (!nostrConnectUri()) void startNostrConnect()
  }

  function copyUri() {
    void navigator.clipboard.writeText(nostrConnectUri())
  }

  createEffect(() => {
    if (!props.inModal && identity()) {
      navigate("/relays")
    }
  })

  onCleanup(() => {
    abortController?.abort()
    scanner?.destroy()
  })

  return (
    <div class={props.inModal ? "w-full" : "min-h-screen bg-gradient-to-br from-gray-50 via-white to-gray-100 flex items-center justify-center p-6"}>
      <div class="w-full max-w-3xl">
        <div class="relative overflow-hidden rounded-2xl border border-gray-200 bg-white shadow-xl">
          <Show when={props.inModal && props.onClose}>
            <button
              type="button"
              class="absolute right-4 top-4 z-10 rounded-md p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700"
              onClick={() => props.onClose?.()}
              aria-label="Close"
            >
              <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12" /></svg>
            </button>
          </Show>
          <div class="absolute inset-0 bg-[radial-gradient(circle_at_top,_rgba(99,102,241,0.08),_transparent_45%)]" />
          <div class="relative grid gap-8 p-8 md:grid-cols-[1.2fr_1fr] md:p-10">
            <div class="space-y-6">
              <div class="inline-flex items-center gap-2 rounded-full border border-indigo-100 bg-indigo-50 px-3 py-1 text-xs font-semibold uppercase tracking-wide text-indigo-700">
                Secure Nostr Login
              </div>
              <div>
                <h1 class="text-2xl font-semibold text-gray-900 py-2">Welcome!</h1>
                <p class="mt-3 text-sm leading-6 text-gray-600">
                  Connect your Nostr account to manage relay hosting, billing, and access in one place.
                </p>
              </div>
              <div class="space-y-3 text-sm text-gray-600">
                <div class="flex items-start gap-3">
                  <span class="mt-1 h-2 w-2 rounded-full bg-indigo-500" />
                  <p>Own your identity with cryptographic sign-in.</p>
                </div>
                <div class="flex items-start gap-3">
                  <span class="mt-1 h-2 w-2 rounded-full bg-indigo-500" />
                  <p>No passwords or email required.</p>
                </div>
                <div class="flex items-start gap-3">
                  <span class="mt-1 h-2 w-2 rounded-full bg-indigo-500" />
                  <p>Get fast access to your relays.</p>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-gray-200 bg-white/80 p-6">
              <Show when={screen() === "tabs"}>
                <h2 class="text-lg font-semibold text-gray-900">Log in / Sign up</h2>
                <p class="mt-2 text-xs text-gray-500">
                  Use any Nostr signer method. New users are automatically onboarded.
                </p>
                <div class="mt-6 space-y-4">
                  <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${tab() === "nip07" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setTab("nip07")}
                      disabled={!window.nostr}
                    >
                      Extension
                    </button>
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${tab() === "nip46" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setTab("nip46")}
                    >
                      Signer
                    </button>
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${tab() === "key" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setTab("key")}
                    >
                      Key
                    </button>
                  </div>

                  <Show when={tab() === "nip07"}>
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={!window.nostr || loading()}
                      onClick={loginWithNip07}
                    >
                      {loading() ? "Connecting..." : <>Continue with extension <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg></>}
                    </button>
                  </Show>

                  <Show when={tab() === "nip46"}>
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium"
                      onClick={enterSignerScreen}
                    >
                      Continue with signer <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
                    </button>
                  </Show>

                  <Show when={tab() === "key"}>
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium"
                      onClick={() => { setError(""); setScreen("key") }}
                    >
                      Continue with key <svg class="inline-block w-4 h-4 ml-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
                    </button>
                  </Show>
                </div>
              </Show>

              <Show when={screen() === "nip46"}>
                <button
                  type="button"
                  class="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-900"
                  onClick={() => { setError(""); setScreen("tabs") }}
                >
                  <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
                  Back
                </button>
                <h2 class="mt-3 text-lg font-semibold text-gray-900">Log in with signer</h2>
                <div class="mt-4 space-y-4">
                  <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${signerTab() === "qr" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setSignerTab("qr")}
                    >
                      Use QR Code
                    </button>
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${signerTab() === "paste" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setSignerTab("paste")}
                    >
                      Paste Link
                    </button>
                  </div>

                  <Show when={signerTab() === "qr"}>
                    <Show when={qrDataUrl()} fallback={
                      <div class="flex items-center justify-center py-8 text-sm text-gray-400">
                        {loading() ? "Generating..." : "Loading QR code..."}
                      </div>
                    }>
                      <img src={qrDataUrl()} alt="Nostrconnect QR code" class="mx-auto rounded-lg" />
                    </Show>
                    <div class="flex rounded-lg border border-gray-300">
                      <input
                        type="text"
                        readOnly
                        value={nostrConnectUri()}
                        class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-xs text-gray-500 bg-transparent focus:outline-none"
                      />
                      <button
                        type="button"
                        class="flex items-center px-3 text-gray-400 hover:text-gray-700"
                        onClick={copyUri}
                        title="Copy link"
                      >
                        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
                      </button>
                    </div>
                  </Show>

                  <Show when={signerTab() === "paste"}>
                    <div class="flex rounded-lg border border-gray-300">
                      <input
                        value={bunkerUrl()}
                        onInput={(e) => setBunkerUrl(e.currentTarget.value)}
                        placeholder="bunker://..."
                        class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-sm bg-transparent focus:outline-none"
                      />
                      <button
                        type="button"
                        class="flex items-center px-3 text-gray-400 hover:text-gray-700"
                        onClick={openScanner}
                        title="Scan QR code"
                      >
                        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
                      </button>
                    </div>
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={loading() || !bunkerUrl().trim()}
                      onClick={loginWithBunker}
                    >
                      Connect to Signer
                    </button>
                  </Show>
                </div>
              </Show>

              <Show when={screen() === "key"}>
                <button
                  type="button"
                  class="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-900"
                  onClick={() => { setError(""); setScreen("tabs") }}
                >
                  <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
                  Back
                </button>
                <h2 class="mt-3 text-lg font-semibold text-gray-900">Log in with key</h2>
                <div class="mt-4 space-y-4">
                  <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${keyTab() === "plaintext" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setKeyTab("plaintext")}
                    >
                      Plaintext
                    </button>
                    <button
                      type="button"
                      class={`flex-1 rounded-md px-3 py-2 text-sm ${keyTab() === "encrypted" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                      onClick={() => setKeyTab("encrypted")}
                    >
                      Encrypted
                    </button>
                  </div>

                  <form
                    class="space-y-3"
                    onSubmit={(e) => {
                      e.preventDefault()
                      void loginWithKeyMaterial()
                    }}
                  >
                    <Show when={keyTab() === "plaintext"}>
                      <input
                        value={nsecValue()}
                        onInput={(e) => setNsecValue(e.currentTarget.value)}
                        placeholder="nsec1..."
                        class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                      />
                    </Show>
                    <Show when={keyTab() === "encrypted"}>
                      <input
                        value={ncryptsecValue()}
                        onInput={(e) => setNcryptsecValue(e.currentTarget.value)}
                        placeholder="ncryptsec1..."
                        class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                      />
                      <input
                        type="password"
                        value={password()}
                        onInput={(e) => setPassword(e.currentTarget.value)}
                        placeholder="Password"
                        class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                      />
                    </Show>
                    <button
                      type="submit"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={loading()}
                    >
                      Log in
                    </button>
                  </form>
                </div>
              </Show>

              <Show when={error()}>
                <p class="mt-4 text-xs text-red-600">{error()}</p>
              </Show>
            </div>
          </div>
        </div>
        <p class="mt-6 text-center text-xs text-gray-500">
          Having trouble? Make sure your signer is unlocked and connected.
        </p>
      </div>

      <Show when={showScanner()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onClick={closeScanner}>
          <div class="relative w-full max-w-sm rounded-2xl bg-white p-4 shadow-xl" onClick={(e) => e.stopPropagation()}>
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-gray-900">Scan QR Code</h3>
              <button type="button" class="text-gray-400 hover:text-gray-700" onClick={closeScanner}>
                <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>
              </button>
            </div>
            <video ref={scannerVideo} class="w-full rounded-lg" />
          </div>
        </div>
      </Show>
    </div>
  )
}
