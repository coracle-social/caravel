import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js"
import { fuzzySearch } from "@/lib/search"
import AdminInvoiceListItem from "@/components/AdminInvoiceListItem"
import PageContainer from "@/components/PageContainer"
import ResourceState from "@/components/ResourceState"
import SearchInput from "@/components/SearchInput"
import useMinLoading from "@/lib/useMinLoading"
import { primeProfiles, useAdminInvoices } from "@/lib/hooks"

export default function AdminInvoiceList() {
  const [query, setQuery] = createSignal("")
  const [invoices] = useAdminInvoices()
  const loading = useMinLoading(() => invoices.loading)

  // Each list item shows its tenant's profile; prime them all in one batch so
  // we don't open a separate outbox subscription per invoice.
  createEffect(() => {
    const list = invoices() ?? []
    if (!list.length) return
    const sub = primeProfiles(list.map(i => i.tenant_pubkey))
    onCleanup(() => sub.unsubscribe())
  })

  const filtered = createMemo(() => {
    const list = invoices() ?? []
    const q = query().trim()
    if (!q) return list
    return fuzzySearch(list, ["tenant_pubkey", "id"], q)
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Invoices</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search invoices..." />
      </div>
      <ResourceState loading={loading()} error={invoices.error} loadingText="Loading invoices..." errorText="Failed to load invoices." />
      <Show when={!loading()}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No invoices found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(invoice) => <AdminInvoiceListItem invoice={invoice} href={`/admin/invoices/${invoice.id}`} showTenant />}
            </For>
          </ul>
        </Show>
      </Show>
    </PageContainer>
  )
}
