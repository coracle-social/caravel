import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "@/shared/components/PageContainer"
import LoadingState from "@/shared/components/LoadingState"
import PaymentDialog from "@/shared/components/PaymentDialog"
import useMinLoading from "@/shared/lib/useMinLoading"
import { useInvoicePdf } from "@/shared/lib/useInvoicePdf"
import PaymentSetupNWC from "@/shared/components/PaymentSetupNWC"
import PaymentSetupCard from "@/shared/components/PaymentSetupCard"
import { useBillingStatus, accountStatus } from "@/shared/lib/billing"
import { account } from "@/shared/lib/account"
import { invoiceStatus, listDraftInvoiceItems, signOut, type Invoice } from "@/shared/lib/api"
import { cardState, methodLabel, nwcState, type PaymentMethodState } from "@/shared/lib/paymentMethod"
import { formatPeriod } from "@/shared/lib/format"
import PaymentMethodRow from "@/shared/components/account/PaymentMethodRow"
import PayoutsSection from "@/tenant/components/PayoutsSection"
import InvoiceListItem from "@/shared/components/account/InvoiceListItem"
import StatusBadge from "@/shared/components/StatusBadge"

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

  // Stripe returns here, so landing settles what was paid there and collects with a new method.
  createEffect(() => {
    const pubkey = account()?.pubkey
    if (pubkey) void billing.autopay(pubkey)
  })

  const status = createMemo(() =>
    accountStatus({
      tenant: billing.tenant(),
      openInvoice: billing.openInvoice(),
      hasPaidSubscription: billing.hasPaidSubscription(),
    }),
  )

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

  async function logout() {
    await signOut()
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
              <StatusBadge status={status()}>Your account is {status()}</StatusBadge>
            </Show>
          </div>

          <ul class="space-y-3">
            <PaymentMethodRow title="Lightning (NWC)" state={nwc()} onAction={() => setNwcModalOpen(true)} />

            <PaymentMethodRow title="Card" state={card()} onAction={() => setCardModalOpen(true)} />
          </ul>
        </section>

        <Show when={account()?.pubkey}>
          {(pubkey) => <PayoutsSection pubkey={pubkey()} />}
        </Show>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Payment History</h2>
          <Show when={invoicesLoading()}>
            <LoadingState message="Loading invoices..." paddingClass="py-8" />
          </Show>
          <Show when={!invoicesLoading()}>
            <Show when={billing.invoices().length > 0 || billing.draftInvoice()} fallback={<p class="text-gray-500 text-sm">No invoices yet.</p>}>
              <ul class="space-y-3">
                <Show when={billing.draftInvoice()}>
                  {(draft) => (
                    <InvoiceListItem
                      isDraft
                      amount={draft().amount}
                      status="draft"
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
                        status={status()}
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
