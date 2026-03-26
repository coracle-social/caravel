import { useParams } from "@solidjs/router"
import { createResource, createSignal, Show } from "solid-js"
import { adminDeactivateRelay, adminGetRelay, adminUpdateRelay, type Relay } from "../../lib/api"
import BackLink from "../../components/BackLink"
import PageContainer from "../../components/PageContainer"
import RelayDetailCard from "../../components/RelayDetailCard"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"

export default function AdminRelayDetail() {
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay, { refetch, mutate }] = createResource(relayId, adminGetRelay)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")
  const loading = useMinLoading(() => relay.loading && !relay())

  async function handleDeactivate() {
    if (busy()) return
    setError("")
    setBusy(true)
    try {
      await adminDeactivateRelay(relayId())
      await refetch()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to deactivate relay")
    } finally {
      setBusy(false)
    }
  }

  function toBool(value: number | undefined, fallback: boolean): boolean {
    if (value === 0) return false
    if (value === 1) return true
    return fallback
  }

  function toInt(value: boolean): number {
    return value ? 1 : 0
  }

  async function updateRelay(next: Relay, previous: Relay) {
    const current = relay()
    if (!current) return

    setError("")
    mutate(next)

    try {
      await adminUpdateRelay(relayId(), next)
      await refetch()
    } catch (e) {
      mutate(previous)
      setError(e instanceof Error ? e.message : "Failed to update relay settings")
    }
  }

  function togglePublicJoin() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      policy_public_join: toInt(!toBool(current.policy_public_join, false)),
    }
    void updateRelay(next, current)
  }

  function toggleStripSignatures() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      policy_strip_signatures: toInt(!toBool(current.policy_strip_signatures, false)),
    }
    void updateRelay(next, current)
  }

  function toggleGroups() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      groups_enabled: toInt(!toBool(current.groups_enabled, true)),
    }
    void updateRelay(next, current)
  }

  function toggleManagement() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      management_enabled: toInt(!toBool(current.management_enabled, true)),
    }
    void updateRelay(next, current)
  }

  function toggleMediaStorage() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      blossom_enabled: toInt(!toBool(current.blossom_enabled, current.plan !== "free")),
    }
    void updateRelay(next, current)
  }

  function togglePushNotifications() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      push_enabled: toInt(!toBool(current.push_enabled, true)),
    }
    void updateRelay(next, current)
  }

  function toggleLivekitSupport() {
    const current = relay()
    if (!current) return
    const next = {
      ...current,
      livekit_enabled: toInt(!toBool(current.livekit_enabled, current.plan !== "free")),
    }
    void updateRelay(next, current)
  }

  return (
    <PageContainer>
      <BackLink href="/admin/relays" label="Relays" />

      <ResourceState
        loading={loading()}
        error={relay.error}
        loadingText="Loading relay..."
        errorText="Failed to load relay."
        class="mb-4"
      />

      <Show when={!loading() && relay()}>
        {(r) => (
          <div class="mb-6">
            <RelayDetailCard
              relay={r()}
              showTenant
              editHref={`/admin/relays/${params.id}/edit`}
              onDeactivate={handleDeactivate}
              deactivating={busy()}
              onTogglePublicJoin={togglePublicJoin}
              onToggleStripSignatures={toggleStripSignatures}
              onToggleGroups={toggleGroups}
              onToggleManagement={toggleManagement}
              onToggleMediaStorage={toggleMediaStorage}
              onToggleLivekitSupport={toggleLivekitSupport}
              onTogglePushNotifications={togglePushNotifications}
              enforcePlanLimits={false}
              showPlanActions={false}
            />
          </div>
        )}
      </Show>
      <Show when={error()}>
        <p class="mt-3 text-sm text-red-600">{error()}</p>
      </Show>
    </PageContainer>
  )
}
