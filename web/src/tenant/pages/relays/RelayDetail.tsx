import { useParams } from "@solidjs/router"
import { Show } from "solid-js"
import ActivityFeed from "@/shared/components/ActivityFeed"
import PageContainer from "@/shared/components/PageContainer"
import PostPaidPrompts, { usePostPaidFlow } from "@/shared/components/PostPaidPrompts"
import RelayDetailCard from "@/shared/components/RelayDetailCard"
import ResourceState from "@/shared/components/ResourceState"
import type { PlanId } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { FREE } from "@/shared/lib/plans"
import { useRelayActivity, useRelayMembers } from "@/shared/lib/relays"
import { setToastMessage } from "@/shared/lib/state"
import useMinLoading from "@/shared/lib/useMinLoading"
import useRelayActions from "@/shared/lib/useRelayActions"
import { useFramedRelay } from "@/tenant/components/RelayFrame"

export default function RelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, actions] = useFramedRelay()
  const [members, { refetch: refetchMembers }] = useRelayMembers(relayId)
  const loading = useMinLoading(() => relay.loading && !relay())
  const [activity] = useRelayActivity(relayId)
  const { busy, deactivate, reactivate, erase, purge, toggle, updatePlan } = useRelayActions(relayId, relay, actions)
  const postPaid = usePostPaidFlow()

  async function changePlan(planId: PlanId) {
    await updatePlan(planId)
    // Not awaited: the card reports the plan change, and the prompts report their own failure.
    if (planId !== FREE) void postPaid.start().catch(e => setToastMessage(formatError(e, "Failed to update billing")))
  }

  return (
    <PageContainer>
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
              onErase={async () => {
                await erase()
                void refetchMembers()
              }}
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
