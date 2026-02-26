import { A } from "@solidjs/router"
import { createResource, For, Show } from "solid-js"
import { listTenantRelays } from "../../lib/api"

export default function RelayList() {
  const [relays] = createResource(listTenantRelays)

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center justify-between mb-6">
        <h1 class="text-2xl font-bold text-gray-900">My Relays</h1>
        <A
          href="/relays/new"
          class="py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 transition-colors"
        >
          Add Relay
        </A>
      </div>

      <Show when={relays.loading}>
        <p class="text-gray-500">Loading relays...</p>
      </Show>

      <Show when={relays.error && !relays.loading}>
        <p class="text-red-600">Failed to load relays.</p>
      </Show>

      <Show when={(relays()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No relays yet. Create one to get started.</p>}>
        <ul class="space-y-3">
          <For each={relays()}>
            {(relay) => (
              <li>
                <A
                  href={`/relays/${relay.id}`}
                  class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300"
                >
                  <div class="flex items-center justify-between">
                    <div>
                      <p class="font-medium text-gray-900">{relay.name}</p>
                      <p class="text-sm text-gray-500">https://{relay.subdomain}.spaces.coracle.social</p>
                    </div>
                    <p class="text-xs uppercase tracking-wide text-gray-500">{relay.status}</p>
                  </div>
                </A>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  )
}
