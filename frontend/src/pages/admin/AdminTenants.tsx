import { A } from "@solidjs/router"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { adminListTenants } from "../../lib/api"

export default function AdminTenants() {
  const [query, setQuery] = createSignal("")
  const [tenants] = createResource(adminListTenants)

  const filtered = createMemo(() => {
    const q = query().trim().toLowerCase()
    return (tenants() ?? []).filter((tenant) => {
      if (!q) return true
      return tenant.pubkey.toLowerCase().includes(q) || tenant.status.toLowerCase().includes(q)
    })
  })

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Tenants</h1>
      <input
        type="search"
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
        placeholder="Search tenants..."
        class="w-full border border-gray-300 rounded-lg px-3 py-2 mb-6 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />

      <Show when={tenants.loading}>
        <p class="text-gray-500">Loading tenants...</p>
      </Show>
      <Show when={tenants.error && !tenants.loading}>
        <p class="text-red-600">Failed to load tenants.</p>
      </Show>
      <Show when={(filtered().length ?? 0) > 0} fallback={<p class="text-gray-500">No tenants found.</p>}>
        <ul class="space-y-3">
          <For each={filtered()}>
            {(tenant) => (
              <li>
                <A href={`/admin/tenants/${tenant.pubkey}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                  <div class="flex items-center justify-between gap-3">
                    <p class="text-sm break-all text-gray-700">{tenant.pubkey}</p>
                    <p class="text-xs uppercase tracking-wide text-gray-500">{tenant.status}</p>
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
