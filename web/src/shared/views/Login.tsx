import { Show, createSignal, onCleanup } from "solid-js"
import { ExtensionAccount, NostrConnectAccount, PasswordAccount, PrivateKeyAccount } from "applesauce-accounts/accounts"
import { PasswordSigner } from "applesauce-signers"
import QrScanner from "qr-scanner"
import QRCode from "qrcode"
import { PLATFORM_NAME } from "@/shared/lib/config"
import { accountManager, ensureSessionTenant } from "@/shared/lib/state"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { formatError } from "@/shared/lib/format"
import { decideKeyLogin, normalizeBunkerUrl, validateBunkerUri, type Tab } from "@/shared/lib/loginInput"
import useMinLoading from "@/shared/lib/useMinLoading"
import LoginTabsScreen from "@/shared/components/login/LoginTabsScreen"
import LoginSignerScreen from "@/shared/components/login/LoginSignerScreen"
import LoginKeyScreen from "@/shared/components/login/LoginKeyScreen"
import QrScannerOverlay from "@/shared/components/login/QrScannerOverlay"

const NIP46_RELAYS = ['wss://bucket.coracle.social', 'wss://ephemeral.snowflare.cc']

type Screen = "tabs" | "nip46" | "key"
type SignerTab = "qr" | "paste"
type KeyTab = "plaintext" | "encrypted"

type LoginProps = {
  inModal?: boolean
  onClose?: () => void
  onAuthenticated?: () => void | Promise<void>
}

async function loadNostrConnectSigner() {
  return import("applesauce-signers").then((m) => m.NostrConnectSigner)
}

// Signing in, as a dialog on the tenant dashboard's home page or as the admin
// dashboard's whole page.
export default function Login(props: LoginProps = {}) {
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
    try {
      await ensureSessionTenant()
    } catch (e) {
      accountManager.removeAccount(account)
      throw e
    }
    await props.onAuthenticated?.()
  }

  // Run one way of logging in, showing it as loading and reporting its failure.
  async function runLogin(fn: () => Promise<void>, fallbackMessage: string) {
    setError("")
    setRawLoading(true)
    try {
      await fn()
    } catch (e) {
      setError(formatError(e, fallbackMessage))
    } finally {
      setRawLoading(false)
    }
  }

  function loginWithNip07() {
    return runLogin(async () => {
      await completeLogin(await ExtensionAccount.fromExtension())
    }, "Failed to login with extension")
  }

  function startNostrConnect() {
    return runLogin(async () => {
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
    }, "Failed to connect signer")
  }

  function loginWithBunker() {
    setError("")
    const validationError = validateBunkerUri(bunkerUrl())
    if (validationError) {
      setError(validationError)
      return
    }
    return runLogin(async () => {
      const uri = normalizeBunkerUrl(bunkerUrl())
      const NostrConnectSigner = await loadNostrConnectSigner()
      const signer = await NostrConnectSigner.fromBunkerURI(uri)
      const pubkey = await signer.getPublicKey()
      const account = new NostrConnectAccount(pubkey, signer)
      await completeLogin(account)
    }, "Invalid bunker URL")
  }

  function loginWithKeyMaterial() {
    return runLogin(async () => {
      const plan = decideKeyLogin({ ncryptsec: ncryptsecValue(), nsec: nsecValue(), password: password() })
      switch (plan.kind) {
        case "error":
          throw new Error(plan.message)
        case "ncryptsec": {
          const signer = await PasswordSigner.fromNcryptsec(plan.ncryptsec, plan.password)
          const pubkey = await signer.getPublicKey()
          await completeLogin(new PasswordAccount(pubkey, signer))
          break
        }
        case "nsec":
          await completeLogin(PrivateKeyAccount.fromKey(plan.key))
          break
      }
    }, "Invalid key")
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
      setError(formatError(e, "Unable to access camera"))
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
    void copyToClipboard(nostrConnectUri(), { successMessage: "Link copied" })
  }

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
                <LoginTabsScreen
                  tab={tab}
                  setTab={setTab}
                  loading={loading}
                  hasExtension={Boolean(window.nostr)}
                  onContinueExtension={loginWithNip07}
                  onContinueSigner={enterSignerScreen}
                  onContinueKey={() => { setError(""); setScreen("key") }}
                />
              </Show>

              <Show when={screen() === "nip46"}>
                <LoginSignerScreen
                  signerTab={signerTab}
                  setSignerTab={setSignerTab}
                  qrDataUrl={qrDataUrl}
                  nostrConnectUri={nostrConnectUri}
                  bunkerUrl={bunkerUrl}
                  setBunkerUrl={setBunkerUrl}
                  loading={loading}
                  onBack={() => { setError(""); setScreen("tabs") }}
                  onCopyUri={copyUri}
                  onScan={openScanner}
                  onConnectBunker={loginWithBunker}
                />
              </Show>

              <Show when={screen() === "key"}>
                <LoginKeyScreen
                  keyTab={keyTab}
                  setKeyTab={setKeyTab}
                  nsecValue={nsecValue}
                  setNsecValue={setNsecValue}
                  ncryptsecValue={ncryptsecValue}
                  setNcryptsecValue={setNcryptsecValue}
                  password={password}
                  setPassword={setPassword}
                  loading={loading}
                  onBack={() => { setError(""); setScreen("tabs") }}
                  onSubmit={() => void loginWithKeyMaterial()}
                />
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

      <QrScannerOverlay open={showScanner()} onClose={closeScanner} videoRef={(el) => { scannerVideo = el }} />
    </div>
  )
}
