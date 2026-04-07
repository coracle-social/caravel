import { createSignal } from "solid-js"
import { updateRelayById, deactivateRelayById, reactivateRelayById, tenantNeedsPaymentSetup, type Relay } from "@/lib/hooks"
import { setToastMessage } from "@/components/Toast"
import type { PlanId } from "@/lib/api"

function toBool(value: number | undefined, fallback: boolean): boolean {
  if (value === 0) return false
  if (value === 1) return true
  return fallback
}

function toInt(value: boolean): number {
  return value ? 1 : 0
}

type RelayResource = {
  (): Relay | undefined
  loading: boolean
  error: unknown
}

type RelayActions = {
  refetch: (...args: unknown[]) => unknown
  mutate: (v: Relay | undefined) => void
}

export default function useRelayToggles(
  relayId: () => string,
  relay: RelayResource,
  { refetch, mutate }: RelayActions,
) {
  const [busy, setBusy] = createSignal(false)
  const [needsPaymentSetup, setNeedsPaymentSetup] = createSignal(false)

  async function updateRelay(next: Relay, previous: Relay) {
    mutate(next)
    try {
      await updateRelayById(relayId(), next)
      await refetch()
    } catch (e) {
      mutate(previous)
      setToastMessage(e instanceof Error ? e.message : "Failed to update relay settings")
    }
  }

  function toggle(field: keyof Relay, fallback: boolean) {
    const current = relay()
    if (!current) return
    const next = { ...current, [field]: toInt(!toBool(current[field] as number, fallback)) }
    void updateRelay(next, current)
  }

  async function handleDeactivate() {
    if (busy()) return
    setBusy(true)
    try {
      await deactivateRelayById(relayId())
      await refetch()
    } catch (e) {
      setToastMessage(e instanceof Error ? e.message : "Failed to deactivate relay")
    } finally {
      setBusy(false)
    }
  }

  async function handleReactivate() {
    if (busy()) return
    setBusy(true)
    try {
      await reactivateRelayById(relayId())
      await refetch()
    } catch (e) {
      setToastMessage(e instanceof Error ? e.message : "Failed to reactivate relay")
    } finally {
      setBusy(false)
    }
  }

  async function handleUpdatePlan(plan: PlanId) {
    const current = relay()
    if (!current) return

    const previous = current
    const next = { ...current, plan }
    const update: Record<string, unknown> = { plan }
    if (plan === "free") {
      next.blossom_enabled = 0
      next.livekit_enabled = 0
      update.blossom_enabled = 0
      update.livekit_enabled = 0
    }
    mutate(next)

    try {
      await updateRelayById(relayId(), update)
      await refetch()
    } catch (e) {
      mutate(previous)
      setToastMessage(e instanceof Error ? e.message : "Failed to update relay plan")
      throw e
    }

    if (plan !== "free") {
      const needs = await tenantNeedsPaymentSetup()
      if (needs) setNeedsPaymentSetup(true)
    }
  }

  const toggles = {
    onTogglePublicJoin: () => toggle("policy_public_join", false),
    onToggleStripSignatures: () => toggle("policy_strip_signatures", false),
    onToggleGroups: () => toggle("groups_enabled", true),
    onToggleManagement: () => toggle("management_enabled", true),
    onToggleMediaStorage: () => toggle("blossom_enabled", relay()?.plan !== "free"),
    onTogglePushNotifications: () => toggle("push_enabled", true),
    onToggleLivekitSupport: () => toggle("livekit_enabled", relay()?.plan !== "free"),
  }

  return { busy, handleDeactivate, handleReactivate, handleUpdatePlan, needsPaymentSetup, clearNeedsPaymentSetup: () => setNeedsPaymentSetup(false), toggles }
}
