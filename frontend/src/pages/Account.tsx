import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "@/components/PageContainer"
import LoadingState from "@/components/LoadingState"
import PaymentDialog from "@/components/PaymentDialog"
import useMinLoading from "@/components/useMinLoading"
import { useInvoicePdf } from "@/lib/useInvoicePdf"
import PaymentSetupNWC from "@/components/PaymentSetupNWC"
import PaymentSetupCard from "@/components/PaymentSetupCard"
import { useBillingStatus, accountStatus, type AccountStatus } from "@/lib/billing"
import { invoiceStatus, listDraftInvoiceItems, type Invoice } from "@/lib/api"
import { account } from "@/lib/state"

const methodLabels: Record<string, string> = {
  nwc: "Lightning",
  stripe: "Card",
  oob: "Lightning",
}

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

type MethodState = { status: "not set up" | "ok" | "error"; error?: string }

const methodStatusStyles: Record<MethodState["status"], string> = {
  "not set up": "bg-gray-100 text-gray-500 border-gray-200",
  ok: "bg-green-50 text-green-700 border-green-200",
  error: "bg-red-50 text-red-700 border-red-200",
}

export default function Account() {
  const billing = useBillingStatus()
  const [selectedInvoice, setSelectedInvoice] = createSignal<Invoice | undefined>()
  const [nwcModalOpen, setNwcModalOpen] = createSignal(false)
  const [cardModalOpen, setCardModalOpen] = createSignal(false)
  const invoicesLoading = useMinLoading(() => billing.loading())
  const { printInvoice, printing } = useInvoicePdf()

  // On landing here (the billing portal returns to this page), refresh billing so
  // a card just added in the portal — which get_tenant syncs onto the tenant — and
  // any cleared error show immediately rather than only on the next reconcile.
  createEffect(() => {
    if (account()?.pubkey) billing.refetch()
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
  // isn't masked by the other.
  const nwcState = createMemo<MethodState>(() => {
    const t = billing.tenant()
    if (!t?.nwc_is_set) return { status: "not set up" }
    if (t.nwc_error) return { status: "error", error: t.nwc_error }
    return { status: "ok" }
  })

  const cardState = createMemo<MethodState>(() => {
    const t = billing.tenant()
    if (!t?.stripe_payment_method_id) return { status: "not set up" }
    if (t.stripe_error) return { status: "error", error: t.stripe_error }
    return { status: "ok" }
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
            <li class="rounded-lg border border-gray-200 p-4">
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0">
                  <p class="text-sm font-medium text-gray-900">Lightning (NWC)</p>
                  <Show when={nwcState().status === "error" && nwcState().error}>
                    <p class="text-xs text-red-600 mt-0.5 break-words">{nwcState().error}</p>
                  </Show>
                </div>
                <div class="flex items-center gap-3 flex-shrink-0">
                  <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${methodStatusStyles[nwcState().status]}`}>
                    {nwcState().status}
                  </span>
                  <button
                    type="button"
                    onClick={() => setNwcModalOpen(true)}
                    class="text-sm font-medium text-blue-600 hover:text-blue-700 disabled:opacity-50"
                  >
                    {nwcState().status === "not set up" ? "Set up" : "Update"}
                  </button>
                </div>
              </div>
            </li>

            {/* Card / Stripe row — CTA opens the card modal (which redirects to the Stripe portal) */}
            <li class="rounded-lg border border-gray-200 p-4">
              <div class="flex items-center justify-between gap-3">
                <div class="min-w-0">
                  <p class="text-sm font-medium text-gray-900">Card</p>
                  <Show when={cardState().status === "error" && cardState().error}>
                    <p class="text-xs text-red-600 mt-0.5 break-words">{cardState().error}</p>
                  </Show>
                </div>
                <div class="flex items-center gap-3 flex-shrink-0">
                  <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${methodStatusStyles[cardState().status]}`}>
                    {cardState().status}
                  </span>
                  <button
                    type="button"
                    onClick={() => setCardModalOpen(true)}
                    class="text-sm font-medium text-blue-600 hover:text-blue-700 disabled:opacity-50"
                  >
                    {cardState().status === "not set up" ? "Set up" : "Update"}
                  </button>
                </div>
              </div>
            </li>
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
                  {(draft) => {
                    const periodLabel = () => {
                      const start = new Date(draft().period_start * 1000)
                      const end = new Date(draft().period_end * 1000)
                      return `${start.toLocaleDateString()} – ${end.toLocaleDateString()}`
                    }
                    return (
                      <li class="rounded-lg border border-dashed border-gray-300 p-4 text-sm">
                        <div class="flex items-center justify-between gap-3">
                          <div class="min-w-0">
                            <div class="flex items-center gap-2">
                              <span class="font-medium text-gray-900">${(draft().amount / 100).toFixed(2)}</span>
                              <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${invoiceStatusStyles.draft}`}>
                                draft
                              </span>
                            </div>
                            <Show when={draft().period_start && draft().period_end}>
                              <p class="text-xs text-gray-500 mt-0.5">{periodLabel()}</p>
                            </Show>
                            <p class="text-xs text-gray-400 mt-1">Charges accruing this period. You'll be invoiced once a balance is due.</p>
                          </div>
                          <div class="flex items-center gap-2 flex-shrink-0">
                            <button
                              type="button"
                              onClick={() => void printInvoice(draft(), () => listDraftInvoiceItems(draft().tenant_pubkey))}
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
                </Show>
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
                      <li class="rounded-lg border border-gray-200 p-4 text-sm">
                        <div class="flex items-center justify-between gap-3">
                          <div class="min-w-0">
                            <div class="flex items-center gap-2">
                              <span class="font-medium text-gray-900">
                                ${(invoice.amount / 100).toFixed(2)}
                              </span>
                              <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${statusStyle()}`}>
                                {status()}
                              </span>
                              <Show when={invoice.method}>
                                <span class="text-xs text-gray-500">· paid via {methodLabels[invoice.method!] ?? invoice.method}</span>
                              </Show>
                            </div>
                            <Show when={invoice.period_start && invoice.period_end}>
                              <p class="text-xs text-gray-500 mt-0.5">{periodLabel()}</p>
                            </Show>
                          </div>
                          <div class="flex items-center gap-2 flex-shrink-0">
                            <Show when={isOpen()}>
                              <button
                                type="button"
                                onClick={() => setSelectedInvoice(invoice)}
                                class="py-1 px-3 bg-blue-600 text-white text-xs font-medium rounded-lg hover:bg-blue-700 transition-colors"
                              >
                                Pay now
                              </button>
                            </Show>
                            <button
                              type="button"
                              onClick={() => void printInvoice(invoice)}
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

      <PaymentSetupNWC
        open={nwcModalOpen()}
        isUpdate={nwcState().status !== "not set up"}
        onClose={() => {
          setNwcModalOpen(false)
          billing.refetch()
        }}
      />

      <PaymentSetupCard
        open={cardModalOpen()}
        isUpdate={cardState().status !== "not set up"}
        onClose={() => {
          setCardModalOpen(false)
          billing.refetch()
        }}
      />
    </PageContainer>
  )
}
