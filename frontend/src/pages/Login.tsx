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
    <div class="min-h-screen bg-gray-50 flex items-center justify-center p-4">
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-8 w-full max-w-md">
        <h1 class="text-2xl font-bold text-gray-900 mb-6 text-center">Log In</h1>
        <div ref={container} />
      </div>
    </div>
  )
}
