import { createSignal } from "solid-js"
import { updateRelayById, deactivateRelayById, reactivateRelayById, resolvePostPaidFlow, type Relay } from "@/lib/hooks"
import { setToastMessage } from "@/lib/state"
import { applyPlanToRelay, planUpdatePayload, toggleField } from "@/lib/relayPlanFlow"
import type { Invoice, PlanId } from "@/lib/api"

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
  const [pendingInvoice, setPendingInvoice] = createSignal<Invoice | undefined>()
  const [pendingPaymentSetup, setPendingPaymentSetup] = createSignal(false)

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
    void updateRelay(toggleField(current, field, fallback), current)
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

  async function handleUpdatePlan(plan_id: PlanId) {
    const current = relay()
    if (!current) return

    const previous = current
    mutate(applyPlanToRelay(current, plan_id))

    try {
      await updateRelayById(relayId(), planUpdatePayload(plan_id))
      await refetch()
    } catch (e) {
      mutate(previous)
      setToastMessage(e instanceof Error ? e.message : "Failed to update relay plan")
      throw e
    }

    if (plan_id === "free") return

    // Paid upgrades materialize an open invoice; resolvePostPaidFlow reconciles
    // and decides whether to prompt the tenant to pay it, set up a payment method,
    // or do nothing when autopay is already configured.
    const decision = await resolvePostPaidFlow()
    switch (decision.kind) {
      case "pay_invoice":
        setPendingInvoice(decision.invoice)
        setPendingPaymentSetup(true)
        break
      case "setup":
        setPendingPaymentSetup(true)
        break
      case "navigate":
        break
    }
  }

  const toggles = {
    onTogglePublicRead: () => toggle("policy_public_read", false),
    onTogglePublicWrite: () => toggle("policy_public_write", false),
    onTogglePublicJoin: () => toggle("policy_public_join", false),
    onToggleStripSignatures: () => toggle("policy_strip_signatures", false),
    onToggleGroups: () => toggle("groups_enabled", true),
    onToggleManagement: () => toggle("management_enabled", true),
    onToggleMediaStorage: () => toggle("blossom_enabled", relay()?.plan_id !== "free"),
    onTogglePushNotifications: () => toggle("push_enabled", true),
    onToggleLivekitSupport: () => toggle("livekit_enabled", relay()?.plan_id !== "free"),
  }

  return { busy, handleDeactivate, handleReactivate, handleUpdatePlan, pendingInvoice, clearPendingInvoice: () => setPendingInvoice(undefined), pendingPaymentSetup, clearPendingPaymentSetup: () => setPendingPaymentSetup(false), toggles }
}
