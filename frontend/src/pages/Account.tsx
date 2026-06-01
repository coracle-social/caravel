import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "@/components/PageContainer"
import LoadingState from "@/components/LoadingState"
import PaymentDialog from "@/components/PaymentDialog"
import BillingPrompts from "@/components/BillingPrompts"
import useMinLoading from "@/components/useMinLoading"
import { useInvoicePdf } from "@/lib/useInvoicePdf"
import { updateActiveTenant } from "@/lib/hooks"
import { useBillingStatus } from "@/lib/billing"
import { createPortalSession, invoiceStatus, type Invoice } from "@/lib/api"
import { account } from "@/lib/state"

const methodLabels: Record<string, string> = {
  nwc: "Lightning",
  stripe: "Card",
  oob: "Lightning",
}

const invoiceStatusStyles: Record<string, string> = {
  open: "bg-yellow-50 text-yellow-700 border-yellow-200",
  paid: "bg-green-50 text-green-700 border-green-200",
  void: "bg-gray-100 text-gray-500 border-gray-200",
}

export default function Account() {
  const billing = useBillingStatus()
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")
  const [selectedInvoice, setSelectedInvoice] = createSignal<Invoice | undefined>()
  const [portalLoading, setPortalLoading] = createSignal(false)
  const invoicesLoading = useMinLoading(() => billing.loading())
  const { printInvoice, printing } = useInvoicePdf()

  // On landing here (the billing portal returns to this page), refresh billing so
  // a card just added in the portal — which get_tenant syncs onto the tenant — and
  // any cleared error show immediately rather than only on the next reconcile.
  createEffect(() => {
    if (account()?.pubkey) billing.refetch()
  })

  // The backend never returns the stored nwc_url (it's private), so the input is
  // write-only: we can only act on a newly entered URL, not prefill the saved one.
  const hasBillingChanges = createMemo(() => nwcUrl().trim().length > 0)

  // The amount to surface: the total of any open invoices, else nothing owed.
  const balance = createMemo(() => {
    const due = billing.balance()
    return due > 0 ? { kind: "due" as const, amount: due } : { kind: "clear" as const, amount: 0 }
  })

  async function saveBilling() {
    setError("")
    setSaving(true)
    try {
      await updateActiveTenant({ nwc_url: nwcUrl().trim() })
      setNwcUrl("")
      billing.refetch()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update billing")
    } finally {
      setSaving(false)
    }
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
        {/* Billing prompts, emphasized contextually on the billing page. */}
        <BillingPrompts variant="inline" />

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3">
            <h2 class="text-lg font-semibold text-gray-900">Account Status</h2>
            <Show when={billing.tenant()}>
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

          {/* Current balance */}
          <div class="mb-4 rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
            <p class="text-xs font-medium text-gray-500 uppercase tracking-wide">Current balance</p>
            <Show
              when={balance().kind === "due"}
              fallback={<p class="text-sm text-gray-600 mt-1">You're all paid up.</p>}
            >
              <p class="text-2xl font-bold text-gray-900 mt-0.5">${(balance().amount / 100).toFixed(2)} <span class="text-sm font-normal text-gray-500">due</span></p>
            </Show>
          </div>

          <p class="text-sm text-gray-600 mb-4">
            Enable automatic payments by providing your Nostr Wallet Connect URL, or add a card via Manage Billing.
          </p>
          <Show when={billing.tenant()?.nwc_is_set}>
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
            <Show when={billing.invoices().length > 0} fallback={<p class="text-gray-500 text-sm">No invoices yet.</p>}>
              <ul class="space-y-3">
                <For each={billing.invoices()}>
                  {(invoice) => {
                    const status = () => invoiceStatus(invoice)
                    const isOpen = () => status() === "open"
                    const statusStyle = () => invoiceStatusStyles[status()] ?? "bg-gray-100 text-gray-500 border-gray-200"
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
                              ${(invoice.amount / 100).toFixed(2)}
                            </span>
                            <Show when={invoice.method}>
                              <span class="text-xs text-gray-500"> · paid via {methodLabels[invoice.method!] ?? invoice.method}</span>
                            </Show>
                            <Show when={invoice.period_start && invoice.period_end}>
                              <p class="text-xs text-gray-500 mt-0.5">{periodLabel()}</p>
                            </Show>
                          </div>
                          <div class="flex items-center gap-2 flex-shrink-0">
                            <Show when={isOpen()}>
                              <span class="text-xs text-blue-600 font-medium">Pay now</span>
                            </Show>
                            <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${statusStyle()}`}>
                              {status()}
                            </span>
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation()
                                void printInvoice(invoice)
                              }}
                              disabled={printing()}
                              class="text-xs font-medium text-gray-500 hover:text-gray-800 underline disabled:opacity-50"
                            >
                              PDF
                            </button>
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

      <Show when={selectedInvoice()}>
        {(invoice) => (
          <PaymentDialog
            invoice={invoice()}
            open={true}
            onClose={() => {
              setSelectedInvoice(undefined)
              billing.refetch()
            }}
          />
        )}
      </Show>
    </PageContainer>
  )
}
