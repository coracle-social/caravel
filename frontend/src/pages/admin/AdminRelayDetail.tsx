import { useParams } from "@solidjs/router"
import { createResource, createSignal, Show } from "solid-js"
import { adminDeactivateRelay, adminGetRelay, adminUpdateRelay, type RelayConfig } from "../../lib/api"
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

  function withDefaults(config?: RelayConfig): RelayConfig {
    return {
      policy: {
        public_join: config?.policy.public_join ?? false,
        strip_signatures: config?.policy.strip_signatures ?? false,
      },
      groups: {
        enabled: config?.groups.enabled ?? true,
        auto_join: config?.groups.auto_join ?? true,
      },
      management: {
        enabled: config?.management.enabled ?? true,
      },
      blossom: {
        enabled: config?.blossom.enabled ?? (relay()?.plan !== "free"),
      },
      push: {
        enabled: config?.push.enabled ?? true,
      },
    }
  }

  async function updateFlags(nextConfig: RelayConfig, previousConfig: RelayConfig) {
    const current = relay()
    if (!current) return

    setError("")
    const optimisticRelay = {
      ...current,
      config: nextConfig,
    }
    mutate(optimisticRelay)

    try {
      await adminUpdateRelay(relayId(), {
        name: current.name,
        subdomain: current.subdomain,
        icon: current.icon,
        description: current.description,
        config: nextConfig,
      })
      await refetch()
    } catch (e) {
      mutate({ ...current, config: previousConfig })
      setError(e instanceof Error ? e.message : "Failed to update relay settings")
    }
  }

  function togglePublicJoin() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      policy: { ...config.policy, public_join: !config.policy.public_join },
    }
    void updateFlags(nextConfig, config)
  }

  function toggleStripSignatures() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      policy: { ...config.policy, strip_signatures: !config.policy.strip_signatures },
    }
    void updateFlags(nextConfig, config)
  }

  function toggleGroups() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      groups: { ...config.groups, enabled: !config.groups.enabled },
    }
    void updateFlags(nextConfig, config)
  }

  function toggleManagement() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      management: { enabled: !config.management.enabled },
    }
    void updateFlags(nextConfig, config)
  }

  function toggleMediaStorage() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      blossom: { enabled: !config.blossom.enabled },
    }
    void updateFlags(nextConfig, config)
  }

  function togglePushNotifications() {
    const config = withDefaults(relay()?.config)
    const nextConfig = {
      ...config,
      push: { enabled: !config.push.enabled },
    }
    void updateFlags(nextConfig, config)
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
              onTogglePushNotifications={togglePushNotifications}
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
