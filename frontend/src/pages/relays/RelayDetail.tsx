import { useParams } from "@solidjs/router"
import { createEffect, createResource, createSignal, onCleanup, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentDialog from "@/components/PaymentDialog"
import PaymentSetup from "@/components/PaymentSetup"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import ActivityFeed from "@/components/ActivityFeed"
import { listRelayMembers } from "@/lib/api"
import { useRelay, useRelayActivity } from "@/lib/hooks"
import useRelayToggles from "@/lib/useRelayToggles"
import { setBillingFlowActive } from "@/lib/billing"
import { refetchBilling } from "@/lib/state"

export default function RelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch, mutate }] = useRelay(relayId)
  const [members] = createResource(relayId, async (id) => {
    if (!id) return []

    try {
      return (await listRelayMembers(id)).members
    } catch {
      return []
    }
  })
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, handleDeactivate, handleReactivate, handleUpdatePlan, pendingInvoice, clearPendingInvoice, pendingPaymentSetup, clearPendingPaymentSetup, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  const [paymentSetupOpen, setPaymentSetupOpen] = createSignal(false)

  createEffect(() => {
    if (pendingPaymentSetup() && !pendingInvoice()) {
      setPaymentSetupOpen(true)
      clearPendingPaymentSetup()
    }
  })

  // Suppress the shared banner's redundant pay/setup prompts while this page's
  // own inline plan-change modals are open.
  createEffect(() => setBillingFlowActive(Boolean(pendingInvoice()) || paymentSetupOpen()))
  onCleanup(() => setBillingFlowActive(false))

  return (
    <PageContainer>
      <BackLink href="/relays" label="Relays" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="space-y-6 mb-6">
            <RelayDetailCard
              relay={r()}
              currentMembers={members()?.length}
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
              void refetchBilling()
            }}
          />
        )}
      </Show>
      <PaymentSetup
        open={paymentSetupOpen()}
        onClose={() => {
          setPaymentSetupOpen(false)
          void refetchBilling()
        }}
      />
    </PageContainer>
  )
}
