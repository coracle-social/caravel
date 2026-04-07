import { useParams } from "@solidjs/router"
import { createMemo, createResource, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentSetup from "@/components/PaymentSetup"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import ActivityFeed from "@/components/ActivityFeed"
import { getRelayMembers, useRelay, useRelayActivity } from "@/lib/hooks"
import useRelayToggles from "@/lib/useRelayToggles"

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
  const { busy, handleDeactivate, handleReactivate, handleUpdatePlan, needsPaymentSetup, clearNeedsPaymentSetup, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  return (
    <PageContainer>
      <BackLink href="/relays" label="Relays" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="space-y-6 mb-6">
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
      <PaymentSetup
        open={needsPaymentSetup()}
        onClose={clearNeedsPaymentSetup}
      />
    </PageContainer>
  )
}
