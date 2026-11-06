import { useParams } from "@solidjs/router"
import { Show } from "solid-js"
import ActivityFeed from "@/shared/components/ActivityFeed"
import BackLink from "@/shared/components/BackLink"
import PageContainer from "@/shared/components/PageContainer"
import RelayDetailCard from "@/shared/components/RelayDetailCard"
import ResourceState from "@/shared/components/ResourceState"
import { useRelay, useRelayActivity, useRelayMembers } from "@/shared/lib/relays"
import { loaded } from "@/shared/lib/resource"
import useMinLoading from "@/shared/lib/useMinLoading"
import useRelayActions from "@/shared/lib/useRelayActions"

export default function AdminRelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, actions] = useRelay(relayId)
  const [members, { refetch: refetchMembers }] = useRelayMembers(relayId)
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, deactivate, reactivate, erase, purge, toggle } = useRelayActions(relayId, relay, actions)

  return (
    <PageContainer>
      <BackLink label="Back" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && loaded(relay)}>
        {(r) => (
          <div class="space-y-6 mb-6">
            <RelayDetailCard
              relay={r()}
              currentMembers={members()?.length}
              showTenant
              editHref={`/relays/${params.id}/edit`}
              busy={busy()}
              onDeactivate={deactivate}
              onReactivate={reactivate}
              onErase={async () => {
                await erase()
                void refetchMembers()
              }}
              onPurge={purge}
              onToggle={toggle}
              enforcePlanLimits={false}
              showPlanActions={false}
              mutateRelay={actions.mutate}
            />
            <ActivityFeed activity={loaded(activity) ?? []} loading={activity.loading} error={activity.error} />
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
