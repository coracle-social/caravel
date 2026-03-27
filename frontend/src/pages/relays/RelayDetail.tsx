import { useParams } from "@solidjs/router"
import { createMemo, createResource, Show } from "solid-js"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayDetailCard from "@/components/RelayDetailCard"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import { getRelayMembers, useRelay } from "@/lib/hooks"
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
  const { busy, handleDeactivate, handleUpdatePlan, toggles } = useRelayToggles(relayId, relay, { refetch, mutate })

  return (
    <PageContainer>
      <BackLink href="/relays" label="Relays" />
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." class="mb-4" />
      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="mb-6">
            <RelayDetailCard
              relay={r()}
              currentMembers={members.length}
              editHref={`/relays/${params.id}/edit`}
              onDeactivate={handleDeactivate}
              deactivating={busy()}
              onUpdatePlan={handleUpdatePlan}
              {...toggles}
            />
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
