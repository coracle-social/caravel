import { Show } from "solid-js"
import { A } from "@solidjs/router"
import { useProfilePicture } from "@/lib/hooks"
import { PLATFORM_NAME, account } from "@/lib/state"

export default function Navbar() {
  const picture = useProfilePicture(() => account()?.pubkey)

  return (
    <nav class="bg-white border-b border-gray-200">
      <div class="max-w-4xl mx-auto px-4 h-14 flex items-center justify-between">
        <A href={account() ? "/relays" : "/"} class="flex items-center gap-2">
          <img src="/caravel.png" alt={PLATFORM_NAME} class="h-8 w-8 rounded-full object-cover" />
          <span class="font-bold text-gray-900 text-lg">{PLATFORM_NAME}</span>
        </A>
        <div class="flex items-center gap-4">
          <Show
            when={account()}
            fallback={
              <A
                href="/login"
                class="text-sm py-1.5 px-4 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
              >
                Log In
              </A>
            }
          >
            <A href="/account">
              <Show
                when={picture()}
                fallback={
                  <div class="h-8 w-8 rounded-full bg-gray-200 flex items-center justify-center">
                    <svg class="w-4 h-4 text-gray-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
                  </div>
                }
              >
                <img
                  src={picture()}
                  alt="Profile"
                  class="h-8 w-8 rounded-full object-cover"
                />
              </Show>
            </A>
          </Show>
        </div>
      </div>
    </nav>
  )
}
