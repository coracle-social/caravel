import { useParams, A } from "@solidjs/router"
import { createResource, createSignal, Show } from "solid-js"
import { adminDeactivateRelay, adminGetRelay } from "../../lib/api"

export default function AdminRelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch }] = createResource(relayId, adminGetRelay)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  async function handleDeactivate() {
    if (busy()) return
    setError("")
    setBusy(true)
    try {
      await adminDeactivateRelay(relayId())
      await refetch()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to deactivate relay")
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href="/admin/relays" class="text-gray-500 hover:text-gray-700">← Relays</A>
      </div>

      <Show when={relay.loading}>
        <p class="text-gray-500 mb-4">Loading relay...</p>
      </Show>
      <Show when={relay.error && !relay.loading}>
        <p class="text-red-600 mb-4">Failed to load relay.</p>
      </Show>

      <Show when={relay()}>
        {(r) => (
          <div class="mb-6">
            <h1 class="text-2xl font-bold text-gray-900 mb-1">{r().name}</h1>
            <p class="text-sm text-gray-500">{r().subdomain}.spaces.coracle.social</p>
            <p class="text-sm text-gray-500 mt-2 break-all">Tenant: {r().tenant}</p>
            <p class="text-sm text-gray-700 mt-2">Plan: <span class="uppercase">{r().plan}</span></p>
            <p class="text-sm text-gray-700">Status: <span class="uppercase">{r().status}</span></p>
            <Show when={r().description.trim()}>
              <p class="mt-3 text-gray-700">{r().description}</p>
            </Show>
          </div>
        )}
      </Show>

      <div class="flex gap-3">
        <A
          href={`/admin/relays/${params.id}/edit`}
          class="py-2 px-4 border border-gray-300 text-gray-700 font-medium rounded-lg hover:bg-gray-50 transition-colors"
        >
          Edit
        </A>
        <button
          class="py-2 px-4 border border-red-300 text-red-600 font-medium rounded-lg hover:bg-red-50 transition-colors disabled:opacity-50"
          onClick={handleDeactivate}
          disabled={busy()}
        >
          {busy() ? "Deactivating..." : "Deactivate"}
        </button>
      </div>
      <Show when={error()}>
        <p class="mt-3 text-sm text-red-600">{error()}</p>
      </Show>
    </div>
  )
}
