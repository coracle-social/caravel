import { onMount, onCleanup } from "solid-js"
import { useNavigate } from "@solidjs/router"
import nb from "nonboard"

export default function Login() {
  const navigate = useNavigate()
  let container: HTMLDivElement | undefined

  onMount(() => {
    const app = nb({
      appUrl: window.location.origin,
      appName: "Nostr Relay Hosting",
      onLogin: () => navigate("/relays"),
      onSignup: () => navigate("/relays"),
      onError: (e: unknown) => console.error("Login error:", e),
      onInfo: (msg: unknown) => console.info("Login info:", msg),
    })

    app.mount(container!)
    onCleanup(() => app.destroy())
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

            <div class="rounded-xl border border-gray-200 bg-white/80 p-6 shadow-sm">
              <h2 class="text-lg font-semibold text-gray-900">Log in with Nostr</h2>
              <p class="mt-2 text-xs text-gray-500">
                New here? Create an account in seconds using your favorite Nostr signer.
              </p>
              <div class="mt-6" ref={container} />
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
