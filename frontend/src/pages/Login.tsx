import { Show, createSignal, onCleanup, onMount } from "solid-js"
import { useNavigate } from "@solidjs/router"
import { ExtensionAccount, NostrConnectAccount, PasswordAccount, PrivateKeyAccount } from "applesauce-accounts/accounts"
import { PasswordSigner } from "applesauce-signers"
import QrScanner from "qr-scanner"
import { activateAccount } from "../lib/nostr"

const NIP46_RELAYS = ['wss://bucket.coracle.social', 'wss://ephemeral.snowflare.cc']

type Tab = "nip07" | "nip46" | "key"

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

export default function Login() {
  const navigate = useNavigate()
  const [tab, setTab] = createSignal<Tab>(window.nostr ? "nip07" : "nip46")
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal("")

  const [nostrConnectUri, setNostrConnectUri] = createSignal("")
  const [showScanner, setShowScanner] = createSignal(false)
  const [bunkerUrl, setBunkerUrl] = createSignal("")
  const [nsecValue, setNsecValue] = createSignal("")
  const [ncryptsecValue, setNcryptsecValue] = createSignal("")
  const [password, setPassword] = createSignal("")

  let scannerVideo: HTMLVideoElement | undefined
  let scanner: QrScanner | undefined
  let abortController: AbortController | undefined

  async function completeLogin(account: ExtensionAccount | NostrConnectAccount | PrivateKeyAccount | PasswordAccount) {
    activateAccount(account)
    navigate("/relays")
  }

  async function loginWithNip07() {
    setError("")
    setLoading(true)
    try {
      await completeLogin(await ExtensionAccount.fromExtension())
    } catch (e) {
      console.error("NIP-07 login failed", e)
      setError(e instanceof Error ? e.message : "Failed to login with extension")
    } finally {
      setLoading(false)
    }
  }

  async function startNostrConnect() {
    setError("")
    setLoading(true)

    try {
      const NostrConnectSigner = await loadNostrConnectSigner()
      const signer = new NostrConnectSigner({ relays: NIP46_RELAYS })
      const uri = signer.getNostrConnectURI({
        name: "Caravel",
        url: window.location.origin,
      })
      setNostrConnectUri(uri)

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
      setLoading(false)
    }
  }

  async function loginWithBunker() {
    setError("")
    setLoading(true)
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
      setLoading(false)
    }
  }

  async function loginWithKeyMaterial() {
    setError("")
    setLoading(true)
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
      setLoading(false)
    }
  }

  async function toggleScanner() {
    const next = !showScanner()
    setShowScanner(next)
    if (!next) {
      scanner?.destroy()
      scanner = undefined
      return
    }

    setError("")
    if (!scannerVideo) return
    scanner = new QrScanner(
      scannerVideo,
      (result) => {
        setBunkerUrl(result.data)
        setShowScanner(false)
        scanner?.destroy()
        scanner = undefined
      },
      { returnDetailedScanResult: true },
    )
    try {
      await scanner.start()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Unable to access camera")
      setShowScanner(false)
      scanner?.destroy()
      scanner = undefined
    }
  }

  onCleanup(() => {
    abortController?.abort()
    scanner?.destroy()
  })

  return (
    <div class="min-h-screen bg-gradient-to-br from-gray-50 via-white to-gray-100 flex items-center justify-center p-6">
      <div class="w-full max-w-3xl">
        <div class="relative overflow-hidden rounded-2xl border border-gray-200 bg-white shadow-xl">
          <div class="absolute inset-0 bg-[radial-gradient(circle_at_top,_rgba(99,102,241,0.08),_transparent_45%)]" />
          <div class="relative grid gap-8 p-8 md:grid-cols-[1.2fr_1fr] md:p-10">
            <div class="space-y-6">
              <div class="inline-flex items-center gap-2 rounded-full border border-indigo-100 bg-indigo-50 px-3 py-1 text-xs font-semibold uppercase tracking-wide text-indigo-700">
                Secure Nostr Login
              </div>
              <div>
                <h1 class="text-3xl font-semibold text-gray-900 md:text-4xl">Welcome back</h1>
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
                  <p>Fast access to your relays and invoices.</p>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-gray-200 bg-white/80 p-6">
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
                    NIP-07
                  </button>
                  <button
                    type="button"
                    class={`flex-1 rounded-md px-3 py-2 text-sm ${tab() === "nip46" ? "bg-gray-900 text-white" : "text-gray-700"}`}
                    onClick={() => setTab("nip46")}
                  >
                    NIP-46
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
                  <div class="space-y-3">
                    <p class="text-xs text-gray-500">
                      {window.nostr
                        ? "Detected browser extension signer."
                        : "No extension detected in this browser."}
                    </p>
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={!window.nostr || loading()}
                      onClick={loginWithNip07}
                    >
                      {loading() ? "Connecting..." : "Continue with extension"}
                    </button>
                  </div>
                </Show>

                <Show when={tab() === "nip46"}>
                  <div class="space-y-3">
                    <button
                      type="button"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={loading()}
                      onClick={startNostrConnect}
                    >
                      Generate nostrconnect QR
                    </button>

                    <Show when={nostrConnectUri()}>
                      <a
                        href={nostrConnectUri()}
                        class="block rounded-lg border border-gray-200 p-3 text-xs text-gray-600 break-all"
                        target="_parent"
                      >
                        {nostrConnectUri()}
                      </a>
                    </Show>

                    <input
                      value={bunkerUrl()}
                      onInput={(e) => setBunkerUrl(e.currentTarget.value)}
                      placeholder="bunker://... or nostrconnect://..."
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                    />
                    <div class="flex gap-2">
                      <button
                        type="button"
                        class="flex-1 rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                        disabled={loading() || !bunkerUrl().trim()}
                        onClick={loginWithBunker}
                      >
                        Connect bunker URL
                      </button>
                      <button
                        type="button"
                        class="rounded-lg border border-gray-300 px-3 py-2 text-sm"
                        onClick={toggleScanner}
                      >
                        {showScanner() ? "Stop" : "Scan"}
                      </button>
                    </div>
                    <Show when={showScanner()}>
                      <video ref={scannerVideo} class="w-full rounded-lg border border-gray-200" />
                    </Show>
                  </div>
                </Show>

                <Show when={tab() === "key"}>
                  <form
                    class="space-y-3"
                    onSubmit={(e) => {
                      e.preventDefault()
                      void loginWithKeyMaterial()
                    }}
                  >
                    <input
                      value={nsecValue()}
                      onInput={(e) => setNsecValue(e.currentTarget.value)}
                      placeholder="nsec1..."
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                    />
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
                      placeholder="Password (for ncryptsec)"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
                    />
                    <button
                      type="submit"
                      class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
                      disabled={loading()}
                    >
                      Continue with key
                    </button>
                  </form>
                </Show>

                <Show when={error()}>
                  <p class="text-xs text-red-600">{error()}</p>
                </Show>
              </div>
              <p class="mt-6 text-xs text-gray-500">
                By continuing, you agree to use NIP-98 authentication for secure access.
              </p>
            </div>
          </div>
        </div>
        <p class="mt-6 text-center text-xs text-gray-500">
          Having trouble? Make sure your signer is unlocked and connected.
        </p>
      </div>
    </div>
  )
}
