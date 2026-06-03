import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "@/components/PageContainer"
import LoadingState from "@/components/LoadingState"
import PaymentDialog from "@/components/PaymentDialog"
import useMinLoading from "@/lib/useMinLoading"
import { useInvoicePdf } from "@/lib/useInvoicePdf"
import PaymentSetupNWC from "@/components/PaymentSetupNWC"
import PaymentSetupCard from "@/components/PaymentSetupCard"
import { useBillingStatus, accountStatus, type AccountStatus } from "@/lib/billing"
import { invoiceStatus, listDraftInvoiceItems, reconcileInvoice, type Invoice } from "@/lib/api"
import { cardState, methodLabel, nwcState, type PaymentMethodState } from "@/lib/paymentMethod"
import { account, setToastMessage } from "@/lib/state"
import { formatPeriod } from "@/lib/format"
import PaymentMethodRow from "@/components/account/PaymentMethodRow"
import InvoiceListItem from "@/components/account/InvoiceListItem"

const invoiceStatusStyles: Record<string, string> = {
  draft: "bg-blue-50 text-blue-700 border-blue-200",
  open: "bg-yellow-50 text-yellow-700 border-yellow-200",
  paid: "bg-green-50 text-green-700 border-green-200",
  void: "bg-gray-100 text-gray-500 border-gray-200",
}

const accountStatusStyles: Record<AccountStatus, string> = {
  active: "bg-green-50 text-green-700 border-green-200",
  inactive: "bg-gray-100 text-gray-500 border-gray-200",
  delinquent: "bg-red-50 text-red-700 border-red-200",
}

const INVOICE_PAGE_SIZE = 10

export default function Account() {
  const billing = useBillingStatus()
  const [selectedInvoice, setSelectedInvoice] = createSignal<Invoice | undefined>()
  const [nwcModalOpen, setNwcModalOpen] = createSignal(false)
  const [cardModalOpen, setCardModalOpen] = createSignal(false)
  const [showAllInvoices, setShowAllInvoices] = createSignal(false)
  const visibleInvoices = createMemo(() => {
    const all = billing.invoices()
    return showAllInvoices() ? all : all.slice(0, INVOICE_PAGE_SIZE)
  })
  const invoicesLoading = useMinLoading(() => billing.loading())
  const { printInvoice, printing } = useInvoicePdf()

  // On landing here (the billing portal returns to this page), run the autopay
  // composite: reconcile the subscription, sync a card just added in the portal,
  // and collect the open invoice if a method is now on file — then refresh. This
  // is what pays the outstanding invoice after the user adds a card and returns.
  createEffect(() => {
    const pubkey = account()?.pubkey
    if (pubkey) void billing.autopay(pubkey)
  })

  // Returning from a per-invoice Stripe Checkout (the success_url carries
  // ?paid_invoice=ID): reconcile that invoice so it flips to paid promptly —
  // autopay above only collects when a recurring method is on file, and a
  // one-off Checkout payment doesn't leave one — then strip the marker.
  createEffect(() => {
    const pubkey = account()?.pubkey
    const paidInvoice = new URLSearchParams(window.location.search).get("paid_invoice")
    if (!pubkey || !paidInvoice) return
    void (async () => {
      try {
        const invoice = await reconcileInvoice(paidInvoice)
        setToastMessage(
          invoice.paid_at != null ? "Payment received. Thank you!" : "Your payment is still processing.",
        )
      } catch (e) {
        setToastMessage(e instanceof Error ? e.message : "Failed to confirm payment")
      } finally {
        const params = new URLSearchParams(window.location.search)
        params.delete("paid_invoice")
        const qs = params.toString()
        window.history.replaceState({}, "", `${window.location.pathname}${qs ? `?${qs}` : ""}`)
        billing.refetch()
      }
    })()
  })

  // Coarse account-health summary for the badge. Same snapshot the inline prompt
  // consumes, so the two can never disagree.
  const status = createMemo(() =>
    accountStatus({
      tenant: billing.tenant(),
      openInvoice: billing.openInvoice(),
      hasPaidSubscription: billing.hasPaidSubscription(),
    }),
  )

  // Per-method state, reported independently so a concurrent error on one method
  // isn't masked by the other. Derived from the shared boundary helpers so the
  // badge surface can't drift from the billing prompts.
  const nwc = createMemo<PaymentMethodState>(() => {
    const t = billing.tenant()
    if (!t) return { kind: "not_set_up" }
    return nwcState(t)
  })

  const card = createMemo<PaymentMethodState>(() => {
    const t = billing.tenant()
    if (!t) return { kind: "not_set_up" }
    return cardState(t)
  })

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
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3 mb-4">
            <h2 class="text-lg font-semibold text-gray-900">Payment Methods</h2>
            <Show when={billing.tenant()}>
              <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${accountStatusStyles[status()]}`}>
                Your account is {status()}
              </span>
            </Show>
          </div>

          <ul class="space-y-3">
            {/* Lightning / NWC row — CTA opens the NWC modal */}
            <PaymentMethodRow title="Lightning (NWC)" state={nwc()} onAction={() => setNwcModalOpen(true)} />

            {/* Card / Stripe row — CTA opens the card modal (which redirects to the Stripe portal) */}
            <PaymentMethodRow title="Card" state={card()} onAction={() => setCardModalOpen(true)} />
          </ul>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Payment History</h2>
          <Show when={invoicesLoading()}>
            <LoadingState message="Loading invoices..." paddingClass="py-8" />
          </Show>
          <Show when={!invoicesLoading()}>
            <Show when={billing.invoices().length > 0 || billing.draftInvoice()} fallback={<p class="text-gray-500 text-sm">No invoices yet.</p>}>
              <ul class="space-y-3">
                {/* Draft: this period's in-progress charges, not yet a real invoice. */}
                <Show when={billing.draftInvoice()}>
                  {(draft) => (
                    <InvoiceListItem
                      isDraft
                      amount={draft().amount}
                      statusLabel="draft"
                      statusStyle={invoiceStatusStyles.draft}
                      periodLabel={formatPeriod(draft().period_start, draft().period_end)}
                      onPrintPdf={() => void printInvoice(draft(), () => listDraftInvoiceItems(draft().tenant_pubkey))}
                      printing={printing()}
                    />
                  )}
                </Show>
                <For each={visibleInvoices()}>
                  {(invoice) => {
                    const status = () => invoiceStatus(invoice)
                    return (
                      <InvoiceListItem
                        amount={invoice.amount}
                        statusLabel={status()}
                        statusStyle={invoiceStatusStyles[status()] ?? "bg-gray-100 text-gray-500 border-gray-200"}
                        periodLabel={formatPeriod(invoice.period_start, invoice.period_end)}
                        method={invoice.method ? methodLabel(invoice.method) : undefined}
                        isOpen={status() === "open"}
                        onPay={() => setSelectedInvoice(invoice)}
                        onPrintPdf={() => void printInvoice(invoice)}
                        printing={printing()}
                      />
                    )
                  }}
                </For>
                <Show when={billing.invoices().length > INVOICE_PAGE_SIZE}>
                  <li>
                    <button
                      type="button"
                      onClick={() => setShowAllInvoices(v => !v)}
                      class="text-sm font-medium text-blue-600 hover:text-blue-700"
                    >
                      {showAllInvoices() ? "Show less" : `Show all (${billing.invoices().length})`}
                    </button>
                  </li>
                </Show>
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
              void billing.autopay(account()!.pubkey)
            }}
          />
        )}
      </Show>

      <PaymentSetupNWC
        open={nwcModalOpen()}
        isUpdate={nwc().kind !== "not_set_up"}
        onClose={() => {
          setNwcModalOpen(false)
          void billing.autopay(account()!.pubkey)
        }}
      />

      <PaymentSetupCard
        open={cardModalOpen()}
        isUpdate={card().kind !== "not_set_up"}
        onClose={() => {
          setCardModalOpen(false)
          void billing.autopay(account()!.pubkey)
        }}
      />
    </PageContainer>
  )
}
