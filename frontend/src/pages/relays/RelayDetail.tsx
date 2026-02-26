import { useParams, A } from "@solidjs/router"
import { createResource, Show } from "solid-js"
import { getTenantRelay } from "../../lib/api"

export default function RelayDetail() {
  const params = useParams()
  const [relay] = createResource(() => params.id, getTenantRelay)

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href="/relays" class="text-gray-500 hover:text-gray-700">← Relays</A>
      </div>

      <Show when={relay.loading}>
        <p class="text-gray-500 mb-4">Loading relay...</p>
      </Show>

      <Show when={relay.error && !relay.loading}>
        <p class="text-red-600 mb-4">Failed to load relay.</p>
      </Show>

      <Show when={relay()}>
        {(loadedRelay) => (
          <div class="mb-6">
            <h1 class="text-2xl font-bold text-gray-900">{loadedRelay().name}</h1>
            <p class="mt-1 text-sm text-gray-500">https://{loadedRelay().subdomain}.spaces.coracle.social</p>
            <Show when={loadedRelay().description.trim()}>
              <p class="mt-3 text-gray-700">{loadedRelay().description}</p>
            </Show>
          </div>
        )}
      </Show>

      <div class="flex gap-3">
        <A
          href={`/relays/${params.id}/edit`}
          class="py-2 px-4 border border-gray-300 text-gray-700 font-medium rounded-lg hover:bg-gray-50 transition-colors"
        >
          Edit
        </A>
        <button class="py-2 px-4 border border-red-300 text-red-600 font-medium rounded-lg hover:bg-red-50 transition-colors">
          Deactivate
        </button>
      </div>
    </div>
  )
}
