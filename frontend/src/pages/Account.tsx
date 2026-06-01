import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import PageContainer from "@/components/PageContainer"
import LoadingState from "@/components/LoadingState"
import PaymentDialog from "@/components/PaymentDialog"
import BillingPrompts from "@/components/BillingPrompts"
import useMinLoading from "@/components/useMinLoading"
import { useInvoicePdf } from "@/lib/useInvoicePdf"
import PaymentSetupNWC from "@/components/PaymentSetupNWC"
import PaymentSetupCard from "@/components/PaymentSetupCard"
import { useBillingStatus, accountStatus, type AccountStatus } from "@/lib/billing"
import { invoiceStatus, type Invoice } from "@/lib/api"
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

  // The amount to surface: the total of any open invoices, else nothing owed.
  const balance = createMemo(() => {
    const due = billing.balance()
    return due > 0 ? { kind: "due" as const, amount: due } : { kind: "clear" as const, amount: 0 }
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
        {/* Billing prompts, emphasized contextually on the billing page. */}
        <BillingPrompts variant="inline" />

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <div class="flex items-center justify-between gap-3 mb-4">
            <h2 class="text-lg font-semibold text-gray-900">Account &amp; Billing</h2>
            <Show when={billing.tenant()}>
              <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${accountStatusStyles[status()]}`}>
                {status()}
              </span>
            </Show>
          </div>

          {/* Current balance (relocated from the old Recurring Billing section, logic unchanged) */}
          <div class="mb-6 rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
            <p class="text-xs font-medium text-gray-500 uppercase tracking-wide">Current balance</p>
            <Show
              when={balance().kind === "due"}
              fallback={<p class="text-sm text-gray-600 mt-1">You're all paid up.</p>}
            >
              <p class="text-2xl font-bold text-gray-900 mt-0.5">${(balance().amount / 100).toFixed(2)} <span class="text-sm font-normal text-gray-500">due</span></p>
            </Show>
          </div>

          {/* Billing methods */}
          <p class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">Billing methods</p>
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
