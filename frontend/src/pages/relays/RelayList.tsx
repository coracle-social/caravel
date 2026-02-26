import { A } from "@solidjs/router"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { listTenantRelays } from "../../lib/api"

export default function RelayList() {
  const [relays] = createResource(listTenantRelays)
  const [query, setQuery] = createSignal("")
  const [status, setStatus] = createSignal("all")

  const filtered = createMemo(() => {
    const q = query().trim().toLowerCase()
    return (relays() ?? []).filter((relay) => {
      const matchesQuery =
        !q ||
        relay.name.toLowerCase().includes(q) ||
        relay.subdomain.toLowerCase().includes(q)
      const matchesStatus = status() === "all" || relay.status === status()
      return matchesQuery && matchesStatus
    })
  })

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

      <div class="mb-6 grid gap-3 sm:grid-cols-[1fr_auto]">
        <input
          type="search"
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          placeholder="Search by name or subdomain"
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
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

      <Show when={relays.loading}>
        <p class="text-gray-500">Loading relays...</p>
      </Show>

      <Show when={relays.error && !relays.loading}>
        <p class="text-red-600">Failed to load relays.</p>
      </Show>

      <Show when={(filtered().length ?? 0) > 0} fallback={<p class="text-gray-500">No relays match your filters.</p>}>
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
