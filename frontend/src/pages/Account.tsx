import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import { getTenant, listTenantInvoices, updateTenantBilling } from "../lib/api"

export default function Account() {
  const [tenant, { refetch: refetchTenant }] = createResource(getTenant)
  const [invoices] = createResource(listTenantInvoices)
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  const recurringEnabled = createMemo(() => !!tenant()?.tenant_nwc_url?.trim())

  async function saveBilling() {
    setError("")
    setSaving(true)
    try {
      await updateTenantBilling(nwcUrl().trim())
      setNwcUrl("")
      await refetchTenant()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update billing")
    } finally {
      setSaving(false)
    }
  }

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Account</h1>

      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Account Status</h2>
          <Show when={tenant.loading}>
            <p class="text-gray-500">Loading account...</p>
          </Show>
          <Show when={tenant()}>
            {(t) => (
              <div class="text-sm text-gray-700 space-y-2">
                <p>
                  Status: <span class="font-medium capitalize">{t().status}</span>
                </p>
                <p class="break-all">Tenant pubkey: {t().pubkey}</p>
              </div>
            )}
          </Show>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Recurring Billing</h2>
          <p class="text-sm text-gray-600 mb-4">
            Enable automatic payments by providing your Nostr Wallet Connect URL.
          </p>
          <p class="text-sm mb-3">
            Current setting:{" "}
            <span class={recurringEnabled() ? "text-green-700 font-medium" : "text-gray-600"}>
              {recurringEnabled() ? "Enabled" : "Disabled"}
            </span>
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
              disabled={saving()}
              class="py-2 px-4 bg-blue-600 text-white rounded-lg disabled:opacity-50"
            >
              {saving() ? "Saving..." : "Save"}
            </button>
            <button
              type="button"
              onClick={() => {
                setNwcUrl("")
                void saveBilling()
              }}
              disabled={saving() || !recurringEnabled()}
              class="py-2 px-4 border border-gray-300 rounded-lg disabled:opacity-50"
            >
              Disable
            </button>
          </div>
          <Show when={error()}>
            <p class="mt-3 text-sm text-red-600">{error()}</p>
          </Show>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Invoice History</h2>
          <Show when={invoices.loading}>
            <p class="text-gray-500">Loading invoices...</p>
          </Show>
          <Show when={(invoices()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No invoices yet.</p>}>
            <ul class="space-y-3">
              <For each={invoices()}>
                {(invoice) => (
                  <li class="rounded-lg border border-gray-200 p-3 text-sm text-gray-700">
                    <div class="flex items-center justify-between gap-3">
                      <span class="font-medium">{invoice.amount.toLocaleString()} sats</span>
                      <span class="uppercase text-xs tracking-wide text-gray-500">{invoice.status}</span>
                    </div>
                    <p class="text-xs text-gray-500 mt-1">{new Date(invoice.created_at).toLocaleString()}</p>
                    <p class="text-xs mt-2 break-all">{invoice.invoice}</p>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </section>
      </div>
    </div>
  )
}
