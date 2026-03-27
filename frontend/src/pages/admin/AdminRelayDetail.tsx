import { useParams } from "@solidjs/router"
import { Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import ActivityFeed from "@/components/ActivityFeed"
import { useRelay, useRelayActivity } from "@/lib/hooks"
import useRelayToggles from "@/lib/useRelayToggles"

export default function AdminRelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch, mutate }] = useRelay(relayId)
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, handleDeactivate, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  return (
    <PageContainer>
      <BackLink href="/admin/relays" label="Relays" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="space-y-6 mb-6">
            <RelayDetailCard
              relay={r()}
              showTenant
              editHref={`/admin/relays/${params.id}/edit`}
              onDeactivate={handleDeactivate}
              deactivating={busy()}
              enforcePlanLimits={false}
              showPlanActions={false}
              {...toggles}
            />
            <ActivityFeed activity={activity() ?? []} loading={activity.loading} />
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
