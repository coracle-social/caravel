import { useParams } from "@solidjs/router"
import { createMemo, createResource, createSignal, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentDialog from "@/components/PaymentDialog"
import PaymentSetup from "@/components/PaymentSetup"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import ActivityFeed from "@/components/ActivityFeed"
import { getLatestOpenInvoice, getRelayMembers, useRelay, useRelayActivity, useTenant } from "@/lib/hooks"
import useRelayToggles from "@/lib/useRelayToggles"
import { plans } from "@/lib/state"

export default function RelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch, mutate }] = useRelay(relayId)
  const relayUrl = createMemo(() => {
    const subdomain = relay()?.subdomain
    return subdomain ? `wss://${subdomain}.spaces.coracle.social` : undefined
  })
  const [members] = createResource(relayUrl, getRelayMembers)
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, handleDeactivate, handleReactivate, handleUpdatePlan, pendingInvoice, clearPendingInvoice, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  const [tenant, { refetch: refetchTenant }] = useTenant()
  const [paymentSetupOpen, setPaymentSetupOpen] = createSignal(false)
  const [invoiceDialogOpen, setInvoiceDialogOpen] = createSignal(false)
  const [paymentBannerDismissed, setPaymentBannerDismissed] = createSignal(false)

  const isPaidRelay = createMemo(() => {
    const r = relay()
    if (!r) return false
    const plan = plans().find(p => p.id === r.plan)
    return !!(plan && plan.amount > 0)
  })

  const [openInvoice, { refetch: refetchOpenInvoice }] = createResource(
    isPaidRelay,
    async (paid) => paid ? getLatestOpenInvoice() : null
  )

  const showPaymentNudge = createMemo(() => {
    if (paymentBannerDismissed()) return false
    if (!isPaidRelay()) return false
    const t = tenant()
    if (!t) return false
    return !t.nwc_url
  })

  return (
    <PageContainer>
      <BackLink href="/relays" label="Relays" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="space-y-6 mb-6">
            <Show when={showPaymentNudge()}>
              <div class="rounded-lg border border-amber-200 bg-amber-50 p-4 flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <p class="text-sm font-medium text-amber-800">Payment setup recommended</p>
                  <p class="text-sm text-amber-700 mt-1">
                    This relay is on a paid plan. Invoices are due when your subscription starts. Set up NWC or Stripe for automatic payments, or pay open invoices via Lightning.
                  </p>
                </div>
                <div class="flex items-center gap-3 shrink-0">
                  <Show when={openInvoice()}>
                    <button
                      type="button"
                      onClick={() => setInvoiceDialogOpen(true)}
                      class="text-sm font-medium text-amber-800 underline hover:text-amber-900 whitespace-nowrap"
                    >
                      Pay invoice
                    </button>
                  </Show>
                  <button
                    type="button"
                    onClick={() => setPaymentSetupOpen(true)}
                    class="text-sm font-medium text-amber-800 underline hover:text-amber-900 whitespace-nowrap"
                  >
                    Set up payments
                  </button>
                  <button
                    type="button"
                    onClick={() => setPaymentBannerDismissed(true)}
                    aria-label="Dismiss"
                    class="text-amber-500 hover:text-amber-800 shrink-0"
                  >
                    <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M18 6L6 18M6 6l12 12" />
                    </svg>
                  </button>
                </div>
              </div>
            </Show>
            <RelayDetailCard
              relay={r()}
              currentMembers={members.length}
              editHref={`/relays/${params.id}/edit`}
              onDeactivate={handleDeactivate}
              onReactivate={handleReactivate}
              deactivating={busy()}
              reactivating={busy()}
              onUpdatePlan={handleUpdatePlan}
              {...toggles}
            />
            <ActivityFeed activity={activity() ?? []} loading={activity.loading} />
          </div>
        )}
      </Show>
      <Show when={pendingInvoice()}>
        {(inv) => (
          <PaymentDialog
            invoice={inv()}
            open={true}
            onClose={() => {
              clearPendingInvoice()
              void refetchTenant()
              void refetchOpenInvoice()
            }}
          />
        )}
      </Show>
      <Show when={openInvoice()}>
        {(inv) => (
          <PaymentDialog
            invoice={inv()!}
            open={invoiceDialogOpen()}
            onClose={() => {
              setInvoiceDialogOpen(false)
              void refetchOpenInvoice()
            }}
          />
        )}
      </Show>
      <PaymentSetup
        open={paymentSetupOpen()}
        onClose={() => {
          setPaymentSetupOpen(false)
          void refetchTenant()
        }}
      />
    </PageContainer>
  )
}
