import Fuse from "fuse.js"
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js"
import PageContainer from "@/components/PageContainer"
import RelayListItem from "@/components/RelayListItem"
import ResourceState from "@/components/ResourceState"
import SearchInput from "@/components/SearchInput"
import useMinLoading from "@/components/useMinLoading"
import { primeProfiles, useAdminRelays } from "@/lib/hooks"

export default function AdminRelayList() {
  const [query, setQuery] = createSignal("")
  const [relays] = useAdminRelays()
  const loading = useMinLoading(() => relays.loading)

  // Each list item shows its tenant's profile; prime them all in one batch so
  // we don't open a separate outbox subscription per relay.
  createEffect(() => {
    const list = relays() ?? []
    if (!list.length) return
    const sub = primeProfiles(list.map(r => r.tenant_pubkey))
    onCleanup(() => sub.unsubscribe())
  })

  const filtered = createMemo(() => {
    const list = relays() ?? []
    const q = query().trim()
    if (!q) return list
    return new Fuse(list, { keys: ["info_name", "subdomain", "tenant_pubkey"], threshold: 0.35, ignoreLocation: true }).search(q).map(r => r.item)
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Relays</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search relays..." />
      </div>
      <ResourceState loading={loading()} error={relays.error} loadingText="Loading relays..." errorText="Failed to load relays." />
      <Show when={!loading()}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No relays found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(relay) => <RelayListItem relay={relay} href={`/admin/relays/${relay.id}`} showTenant />}
            </For>
          </ul>
        </Show>
      </Show>
    </PageContainer>
  )
}
