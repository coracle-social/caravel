import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js"
import { getTenant, listTenantInvoices, updateTenantBilling } from "../lib/api"
import PageContainer from "../components/PageContainer"
import LoadingState from "../components/LoadingState"
import useMinLoading from "../components/useMinLoading"

export default function Account() {
  const [tenant, { refetch: refetchTenant }] = createResource(getTenant)
  const [invoices] = createResource(listTenantInvoices)
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")
  const invoicesLoading = useMinLoading(() => invoices.loading)

  const hasBillingChanges = createMemo(() => {
    const current = tenant()?.nwc_url?.trim() ?? ""
    const next = nwcUrl().trim()
    return current !== next
  })

  createEffect(() => {
    setNwcUrl(tenant()?.nwc_url ?? "")
  })

  async function saveBilling() {
    setError("")
    setSaving(true)
    try {
      const next = nwcUrl().trim()
      await updateTenantBilling(next)
      await refetchTenant()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update billing")
    } finally {
      setSaving(false)
    }
  }

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">My Account</h1>

      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3">
            <h2 class="text-lg font-semibold text-gray-900">Account Status</h2>
            <Show when={tenant()}>
              {(t) => (
                <span class="rounded-full border border-gray-300 bg-gray-100 px-2.5 py-1 text-xs font-medium uppercase tracking-wide text-gray-700">
                  tenant
                </span>
              )}
            </Show>
          </div>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Recurring Billing</h2>
          <p class="text-sm text-gray-600 mb-4">
            Enable automatic payments by providing your Nostr Wallet Connect URL.
          </p>
          <div class="flex gap-2">
            <input
              type="text"
              value={nwcUrl()}
              onInput={(e) => setNwcUrl(e.currentTarget.value)}
              placeholder="nostr+walletconnect://..."
              class="flex-1 border border-gray-300 rounded-lg px-3 py-2"
            />
            <button
              type="button"
              onClick={saveBilling}
              disabled={saving() || !hasBillingChanges()}
              class="py-2 px-4 bg-blue-600 text-white rounded-lg disabled:opacity-50"
            >
              {saving() ? "Saving..." : "Save"}
            </button>
          </div>
          <Show when={error()}>
            <p class="mt-3 text-sm text-red-600">{error()}</p>
          </Show>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Invoice History</h2>
          <Show when={invoicesLoading()}>
            <LoadingState message="Loading invoices..." paddingClass="py-8" />
          </Show>
          <Show when={!invoicesLoading()}>
            <Show when={(invoices()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No invoices yet.</p>}>
              <ul class="space-y-3">
                <For each={invoices()}>
                  {(invoice) => (
                    <li class="rounded-lg border border-gray-200 p-3 text-sm text-gray-700">
                      <div class="flex items-center justify-between gap-3">
                        <span class="font-medium">—</span>
                        <span class="uppercase text-xs tracking-wide text-gray-500">{invoice.status}</span>
                      </div>
                      <p class="text-xs text-gray-500 mt-1">{new Date(invoice.created_at * 1000).toLocaleString()}</p>
                      <p class="text-xs mt-2 break-all">{invoice.bolt11}</p>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </section>
      </div>
    </PageContainer>
  )
}
