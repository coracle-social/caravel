import { useParams } from "@solidjs/router"
import { createResource, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/lib/useMinLoading"
import ActivityFeed from "@/components/ActivityFeed"
import { listRelayMembers } from "@/lib/api"
import { useRelay, useRelayActivity } from "@/lib/hooks"
import useRelayToggles from "@/lib/useRelayToggles"

export default function AdminRelayDetail() {
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
  const { busy, handleDeactivate, handleReactivate, handlePurge, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  return (
    <PageContainer>
      <BackLink label="Back" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="space-y-6 mb-6">
            <RelayDetailCard
              relay={r()}
              currentMembers={members()?.length}
              showTenant
              editHref={`/admin/relays/${params.id}/edit`}
              onDeactivate={handleDeactivate}
              onReactivate={handleReactivate}
              deactivating={busy()}
              reactivating={busy()}
              onPurge={handlePurge}
              purging={busy()}
              enforcePlanLimits={false}
              showPlanActions={false}
              mutateRelay={mutate}
              {...toggles}
            />
            <ActivityFeed activity={activity() ?? []} loading={activity.loading} />
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
