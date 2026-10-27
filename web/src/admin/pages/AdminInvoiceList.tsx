import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { fuzzySearch } from "@/shared/lib/search"
import AdminInvoiceListItem from "@/admin/components/AdminInvoiceListItem"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import SearchInput from "@/shared/components/SearchInput"
import useMinLoading from "@/shared/lib/useMinLoading"
import { listInvoices } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import { usePrimeProfiles } from "@/shared/lib/profiles"

export default function AdminInvoiceList() {
  const [query, setQuery] = createSignal("")
  const [invoices] = createResource(listInvoices)
  const loading = useMinLoading(() => invoices.loading)

  usePrimeProfiles(() => (loaded(invoices) ?? []).map(i => i.tenant_pubkey))

  const filtered = createMemo(() => {
    return fuzzySearch(loaded(invoices) ?? [], ["tenant_pubkey", "id"], query().trim())
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Invoices</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search invoices..." />
      </div>
      <ResourceState loading={loading()} error={invoices.error} loadingText="Loading invoices..." errorText="Failed to load invoices." />
      <Show when={!loading() && loaded(invoices)}>
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
