import { A } from "@solidjs/router"

export default function Navbar() {
  return (
    <nav class="bg-white border-b border-gray-200">
      <div class="max-w-5xl mx-auto px-4 h-14 flex items-center justify-between">
        <A href="/" class="font-bold text-gray-900 text-lg">
          Relay Hosting
        </A>
        <div class="flex items-center gap-4">
          <A href="/relays" class="text-sm text-gray-600 hover:text-gray-900">
            Dashboard
          </A>
          <A
            href="/login"
            class="text-sm py-1.5 px-4 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
          >
            Log In
          </A>
        </div>
      </div>
    </nav>
  )
}
