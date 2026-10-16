import { useParams } from "@solidjs/router"
import { Show } from "solid-js"
import ActivityFeed from "@/shared/components/ActivityFeed"
import BackLink from "@/shared/components/BackLink"
import PageContainer from "@/shared/components/PageContainer"
import PostPaidPrompts, { usePostPaidFlow } from "@/shared/components/PostPaidPrompts"
import RelayDetailCard from "@/shared/components/RelayDetailCard"
import ResourceState from "@/shared/components/ResourceState"
import type { PlanId } from "@/shared/lib/api"
import { FREE } from "@/shared/lib/plans"
import { useRelay, useRelayActivity, useRelayMembers } from "@/shared/lib/relays"
import useMinLoading from "@/shared/lib/useMinLoading"
import useRelayActions from "@/shared/lib/useRelayActions"

export default function RelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, actions] = useRelay(relayId)
  const [members] = useRelayMembers(relayId)
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, deactivate, reactivate, purge, toggle, updatePlan } = useRelayActions(relayId, relay, actions)
  const postPaid = usePostPaidFlow()

  async function changePlan(planId: PlanId) {
    await updatePlan(planId)
    if (planId !== FREE) await postPaid.start()
  }

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
              editHref={`/relays/${params.id}/edit`}
              busy={busy()}
              onDeactivate={deactivate}
              onReactivate={reactivate}
              onPurge={purge}
              onToggle={toggle}
              onUpdatePlan={changePlan}
              mutateRelay={actions.mutate}
            />
            <ActivityFeed activity={activity() ?? []} loading={activity.loading} />
          </div>
        )}
      </Show>
      <PostPaidPrompts flow={postPaid} />
    </PageContainer>
  )
}
