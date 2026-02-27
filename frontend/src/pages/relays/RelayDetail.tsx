import { useParams, A } from "@solidjs/router"
import { createResource, createSignal, Show } from "solid-js"
import { deactivateTenantRelay, getTenantRelay } from "../../lib/api"
import BackLink from "../../components/BackLink"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"

export default function RelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch }] = createResource(relayId, getTenantRelay)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")
  const loading = useMinLoading(() => relay.loading)

  async function handleDeactivate() {
    if (busy()) return
    setError("")
    setBusy(true)
    try {
      await deactivateTenantRelay(relayId())
      await refetch()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to deactivate relay")
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageContainer>
      <BackLink href="/relays" label="Relays" />

      <ResourceState
        loading={loading()}
        error={relay.error}
        loadingText="Loading relay..."
        errorText="Failed to load relay."
        class="mb-4"
      />

      <Show when={!loading() && relay()}>
        {(loadedRelay) => (
          <div class="mb-6">
            <h1 class="text-2xl font-bold text-gray-900 py-4">{loadedRelay().name}</h1>
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
    </PageContainer>
  )
}
