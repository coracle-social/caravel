import { A } from "@solidjs/router"
import Fuse from "fuse.js"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { listTenantRelays } from "../../lib/api"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"

export default function RelayList() {
  const [relays] = createResource(listTenantRelays)
  const [query, setQuery] = createSignal("")
  const [status, setStatus] = createSignal("all")
  const loading = useMinLoading(() => relays.loading)

  const filtered = createMemo(() => {
    const list = relays() ?? []
    const q = query().trim()
    const searched = q
      ? new Fuse(list, {
          keys: ["info_name", "subdomain"],
          threshold: 0.35,
          ignoreLocation: true,
        }).search(q).map((result) => result.item)
      : list

    return searched.filter((relay) => {
      const matchesStatus = status() === "all" || relay.status === status()
      return matchesStatus
    })
  })

  return (
    <PageContainer>
      <div class="flex items-center justify-between mb-6">
        <h1 class="text-2xl font-bold text-gray-900 py-2">My Relays</h1>
        <A
          href="/relays/new"
          class="py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 transition-colors"
        >
          Add Relay
        </A>
      </div>

      <div class="mb-6 grid gap-3 sm:grid-cols-[1fr_auto]">
        <div class="relative">
          <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-gray-400">
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <circle cx="11" cy="11" r="8" />
              <path d="M21 21l-4.3-4.3" />
            </svg>
          </span>
          <input
            type="search"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            placeholder="Search by name or subdomain"
            class="w-full border border-gray-300 rounded-lg py-2 pl-10 pr-3"
          />
        </div>
        <select
          value={status()}
          onChange={(e) => setStatus(e.currentTarget.value)}
          class="border border-gray-300 rounded-lg px-3 py-2 bg-white"
        >
          <option value="all">All statuses</option>
          <option value="active">Active</option>
          <option value="pending">Pending</option>
          <option value="deactivated">Deactivated</option>
          <option value="provisioning_failed">Provisioning failed</option>
          <option value="suspended">Suspended</option>
        </select>
      </div>

      <ResourceState
        loading={loading()}
        error={relays.error}
        loadingText="Loading relays..."
        errorText="Failed to load relays."
      />

      <Show when={!loading()}>
        <Show when={(filtered().length ?? 0) > 0} fallback={<p class="py-20 text-center text-gray-500">No relays found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(relay) => (
                <li>
                  <A
                    href={`/relays/${relay.id}`}
                    class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300"
                  >
                    <div class="flex items-center justify-between">
                      <div>
                        <p class="font-medium text-gray-900">{relay.info_name || relay.subdomain}</p>
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
      </Show>
    </PageContainer>
  )
}
