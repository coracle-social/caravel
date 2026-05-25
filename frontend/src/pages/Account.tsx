import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js"
import { useSearchParams } from "@solidjs/router"
import PageContainer from "@/components/PageContainer"
import LoadingState from "@/components/LoadingState"
import PaymentDialog from "@/components/PaymentDialog"
import useMinLoading from "@/components/useMinLoading"
import { updateActiveTenant, useTenant } from "@/lib/hooks"
import { createPortalSession, getInvoice, listTenantInvoices, type Invoice } from "@/lib/api"
import { account } from "@/lib/state"

export default function Account() {
  const [tenant, { refetch: refetchTenant }] = useTenant()
  const [invoices, { refetch: refetchInvoices }] = createResource(() => listTenantInvoices(account()!.pubkey))
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")
  const [selectedInvoice, setSelectedInvoice] = createSignal<Invoice | undefined>()
  const [portalLoading, setPortalLoading] = createSignal(false)
  const invoicesLoading = useMinLoading(() => invoices.loading)

  // Deep link: /account?invoice=<id> (e.g. from the billing DM) fetches that
  // invoice and opens the payment dialog. The fetched invoice takes precedence
  // over a row the user clicked in the list.
  const [searchParams, setSearchParams] = useSearchParams()
  const [deepLinkedInvoice] = createResource(
    () => searchParams.invoice as string | undefined,
    (id) => getInvoice(id),
  )
  createEffect(() => {
    if (deepLinkedInvoice.error) setError("Couldn't load that invoice. It may no longer be available.")
  })
  const activeInvoice = () => selectedInvoice() ?? deepLinkedInvoice()

  // The backend never returns the stored nwc_url (it's private), so the input is
  // write-only: we can only act on a newly entered URL, not prefill the saved one.
  const hasBillingChanges = createMemo(() => nwcUrl().trim().length > 0)

  async function saveBilling() {
    setError("")
    setSaving(true)
    try {
      const next = nwcUrl().trim()
      await updateActiveTenant({ nwc_url: next })
      setNwcUrl("")
      await refetchTenant()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update billing")
    } finally {
      setSaving(false)
    }
  }

  function handleInvoiceDialogClose() {
    setSelectedInvoice(undefined)
    // Clearing the query param drops the deep-linked invoice and closes the dialog.
    if (searchParams.invoice) setSearchParams({ invoice: undefined })
    void refetchInvoices()
  }

  async function openPortal() {
    setPortalLoading(true)
    try {
      const { url } = await createPortalSession(account()!.pubkey, window.location.href)
      window.location.href = url
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to open billing portal")
    } finally {
      setPortalLoading(false)
    }
  }

  function logout() {
    localStorage.clear()
    window.location.href = "/"
  }

  const invoiceStatusStyles: Record<string, string> = {
    draft: "bg-gray-100 text-gray-500 border-gray-200",
    open: "bg-yellow-50 text-yellow-700 border-yellow-200",
    paid: "bg-green-50 text-green-700 border-green-200",
    void: "bg-gray-100 text-gray-500 border-gray-200",
    uncollectible: "bg-red-50 text-red-700 border-red-200",
  }

  return (
    <PageContainer>
      <div class="mb-6 py-2 flex items-center justify-between gap-3">
        <h1 class="text-2xl font-bold text-gray-900">My Account</h1>
        <button
          type="button"
          onClick={logout}
          class="py-1.5 px-3 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
        >
          Log out
        </button>
      </div>

      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3">
            <h2 class="text-lg font-semibold text-gray-900">Account Status</h2>
            <Show when={tenant()}>
              <span class="rounded-full border border-gray-300 bg-gray-100 px-2.5 py-1 text-xs font-medium uppercase tracking-wide text-gray-700">
                tenant
              </span>
            </Show>
          </div>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3 mb-4">
            <h2 class="text-lg font-semibold text-gray-900">Recurring Billing</h2>
            <button
              type="button"
              onClick={openPortal}
              disabled={portalLoading()}
              class="text-sm font-medium text-blue-600 hover:text-blue-700 disabled:opacity-50"
            >
              {portalLoading() ? "Loading..." : "Manage Billing"}
            </button>
          </div>
          <p class="text-sm text-gray-600 mb-4">
            Enable automatic payments by providing your Nostr Wallet Connect URL.
          </p>
          <Show when={tenant()?.nwc_is_set}>
            <p class="text-sm text-green-700 mb-4">A wallet is connected. Enter a new URL to replace it.</p>
          </Show>
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
          <Show when={tenant()?.nwc_error}>
            <p class="mt-3 text-sm text-red-600">{tenant()!.nwc_error}</p>
          </Show>
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
            <Show when={(invoices()?.length ?? 0) > 0} fallback={<p class="text-gray-500 text-sm">No invoices yet.</p>}>
              <ul class="space-y-3">
                <For each={invoices()}>
                  {(invoice) => {
                    const isOpen = () => invoice.status === "open"
                    const statusStyle = () => invoiceStatusStyles[invoice.status] ?? "bg-gray-100 text-gray-500 border-gray-200"
                    const periodLabel = () => {
                      const start = new Date(invoice.period_start * 1000)
                      const end = new Date(invoice.period_end * 1000)
                      return `${start.toLocaleDateString()} – ${end.toLocaleDateString()}`
                    }

                    return (
                      <li
                        class={`rounded-lg border border-gray-200 p-4 text-sm ${isOpen() ? "cursor-pointer hover:border-blue-300 hover:bg-blue-50 transition-colors" : ""}`}
                        onClick={() => isOpen() && setSelectedInvoice(invoice)}
                        title={isOpen() ? "Click to pay this invoice" : undefined}
                      >
                        <div class="flex items-center justify-between gap-3">
                          <div>
                            <span class="font-medium text-gray-900">
                              ${(invoice.amount_due / 100).toFixed(2)}
                            </span>
                            <Show when={invoice.period_start && invoice.period_end}>
                              <p class="text-xs text-gray-500 mt-0.5">{periodLabel()}</p>
                            </Show>
                          </div>
                          <div class="flex items-center gap-2 flex-shrink-0">
                            <Show when={isOpen()}>
                              <span class="text-xs text-blue-600 font-medium">Pay now</span>
                            </Show>
                            <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${statusStyle()}`}>
                              {invoice.status}
                            </span>
                          </div>
                        </div>
                      </li>
                    )
                  }}
                </For>
              </ul>
            </Show>
          </Show>
        </section>
      </div>

      <Show when={activeInvoice()}>
        {(invoice) => (
          <PaymentDialog
            invoice={invoice()}
            open={true}
            onClose={handleInvoiceDialogClose}
          />
        )}
      </Show>
    </PageContainer>
  )
}
