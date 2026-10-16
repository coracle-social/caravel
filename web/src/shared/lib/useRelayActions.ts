import { createSignal, type Resource, type ResourceActions } from "solid-js"
import { deactivateRelay, purgeRelay, reactivateRelay, updateRelay, type PlanId, type Relay, type RelayFlag, type RelaySettings } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { FREE } from "@/shared/lib/plans"
import { setToastMessage } from "@/shared/lib/state"

// What a relay's page can do to it, keeping the page's copy current and toasting failures.
export default function useRelayActions(
  relayId: () => string,
  relay: Resource<Relay | undefined>,
  { refetch, mutate }: ResourceActions<Relay | undefined>,
) {
  const [busy, setBusy] = createSignal(false)

  async function run(action: (id: string) => Promise<void>, failure: string, success?: string) {
    if (busy()) return
    setBusy(true)
    try {
      await action(relayId())
      await refetch()
      if (success) setToastMessage(success, "success")
    } catch (e) {
      setToastMessage(formatError(e, failure))
    } finally {
      setBusy(false)
    }
  }

  async function toggle(flag: RelayFlag) {
    const current = relay()
    if (!current) return

    // Only the flag is sent, so settings changed elsewhere since this copy aren't undone.
    const settings: RelaySettings = { [flag]: current[flag] ? 0 : 1 }
    mutate({ ...current, ...settings })
    try {
      await updateRelay(relayId(), settings)
      await refetch()
    } catch (e) {
      mutate(current)
      setToastMessage(formatError(e, "Failed to update relay settings"))
    }
  }

  async function updatePlan(plan_id: PlanId) {
    const current = relay()
    if (!current) return

    // Free relays have no media storage or calls.
    const settings: RelaySettings = plan_id === FREE ? { plan_id, blossom_enabled: 0, livekit_enabled: 0 } : { plan_id }
    mutate({ ...current, ...settings })
    try {
      await updateRelay(relayId(), settings)
      await refetch()
    } catch (e) {
      mutate(current)
      setToastMessage(formatError(e, "Failed to update relay plan"))
      throw e
    }
  }

  return {
    busy,
    toggle,
    updatePlan,
    deactivate: () => run(deactivateRelay, "Failed to deactivate relay"),
    reactivate: () => run(reactivateRelay, "Failed to reactivate relay"),
    purge: () => run(purgeRelay, "Failed to purge relay", "Relay purged"),
  }
}
