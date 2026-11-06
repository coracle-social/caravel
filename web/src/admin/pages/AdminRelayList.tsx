import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { fuzzySearch } from "@/shared/lib/search"
import PageContainer from "@/shared/components/PageContainer"
import RelayListItem from "@/shared/components/RelayListItem"
import ResourceState from "@/shared/components/ResourceState"
import SearchInput from "@/shared/components/SearchInput"
import useMinLoading from "@/shared/lib/useMinLoading"
import { listRelays } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import { usePrimeProfiles } from "@/shared/lib/profiles"

export default function AdminRelayList() {
  const [query, setQuery] = createSignal("")
  const [relays] = createResource(listRelays)
  const loading = useMinLoading(() => relays.loading)

  usePrimeProfiles(() => (loaded(relays) ?? []).map(r => r.tenant_pubkey))

  const filtered = createMemo(() => {
    return fuzzySearch(loaded(relays) ?? [], ["info_name", "subdomain", "tenant_pubkey"], query().trim())
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Relays</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search relays..." />
      </div>
      <ResourceState loading={loading()} error={relays.error} loadingText="Loading relays..." errorText="Failed to load relays." />
      <Show when={!loading() && loaded(relays)}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No relays found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(relay) => <RelayListItem relay={relay} href={`/relays/${relay.id}`} showTenant />}
            </For>
          </ul>
        </Show>
      </Show>
    </PageContainer>
  )
}
