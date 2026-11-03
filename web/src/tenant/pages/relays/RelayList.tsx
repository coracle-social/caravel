import { A } from "@solidjs/router"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import PageContainer from "@/shared/components/PageContainer"
import RelayListItem from "@/shared/components/RelayListItem"
import ResourceState from "@/shared/components/ResourceState"
import SearchInput from "@/shared/components/SearchInput"
import { account } from "@/shared/lib/account"
import { getPayoutStatus } from "@/shared/lib/api"
import useMinLoading from "@/shared/lib/useMinLoading"
import { useTenantRelays } from "@/shared/lib/relays"
import { fuzzySearch } from "@/shared/lib/search"

export default function RelayList() {
  const [relays] = useTenantRelays()
  // Only a relay that charges members needs its tenant's payouts, and asking
  // Stripe costs a call.
  const sells = () => (relays() ?? []).some((relay) => relay.member_price > 0)
  const [connect] = createResource(() => (sells() ? account()!.pubkey : undefined), getPayoutStatus)
  const [query, setQuery] = createSignal("")
  const [status, setStatus] = createSignal("all")
  const loading = useMinLoading(() => relays.loading)

  const filtered = createMemo(() => {
    const searched = fuzzySearch(relays() ?? [], ["info_name", "subdomain"], query().trim())
    return status() === "all" ? searched : searched.filter(r => r.status === status())
  })

  return (
    <PageContainer>
      <div class="flex items-center justify-between mb-6">
        <h1 class="text-2xl font-bold text-gray-900 py-2">My Relays</h1>
        <A href="/relays/new" class="py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 transition-colors">Add Relay</A>
      </div>
      <div class="mb-6 grid gap-3 sm:grid-cols-[1fr_auto]">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search by name or subdomain" />
        <select value={status()} onChange={e => setStatus(e.currentTarget.value)} class="border border-gray-300 rounded-lg px-3 py-2 bg-white">
          <option value="all">All</option>
          <option value="active">Active</option>
          <option value="inactive">Inactive</option>
        </select>
      </div>
      <ResourceState loading={loading()} error={relays.error} loadingText="Loading relays..." errorText="Failed to load relays." />
      <Show when={!loading()}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No relays found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(relay) => (
                <RelayListItem relay={relay} href={`/relays/${relay.id}`} payoutsReady={connect()?.ready} />
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </PageContainer>
  )
}
