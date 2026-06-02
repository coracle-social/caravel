import { Show, createSignal } from "solid-js"
import type { Relay, PlanId } from "@/lib/api"
import ConfirmDialog from "@/components/ConfirmDialog"
import Field from "@/components/Field"
import PricingTable from "@/components/PricingTable"
import ToggleButton from "@/components/ToggleButton"
import ToggleField from "@/components/ToggleField"
import RelayCardHeader from "@/components/relay/RelayCardHeader"
import PlanGatedToggle from "@/components/relay/PlanGatedToggle"
import { setToastMessage } from "@/lib/state"
import { useProfileMetadata } from "@/lib/hooks"
import { flagToBool } from "@/lib/relayFlags"
import { plans } from "@/lib/state"

function DetailSection(props: { title: string; children: any }) {
  return (
    <div>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-6">{props.title}</h3>
      <dl class="grid lg:grid-cols-2 gap-x-32 gap-y-4">
        {props.children}
      </dl>
    </div>
  )
}

function MembershipSection(props: { title: string; children: any }) {
  return (
    <div>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-6">{props.title}</h3>
      <dl class="grid gap-x-6 gap-y-4 sm:grid-cols-2">
        {props.children}
      </dl>
    </div>
  )
}

type RelayDetailCardProps = {
  relay: Relay
  currentMembers?: number
  showTenant?: boolean
  editHref?: string
  onDeactivate?: () => void | Promise<void>
  onReactivate?: () => void | Promise<void>
  deactivating?: boolean
  reactivating?: boolean
  onTogglePublicJoin?: () => void
  onToggleStripSignatures?: () => void
  onToggleGroups?: () => void
  onToggleManagement?: () => void
  onToggleMediaStorage?: () => void
  onToggleLivekitSupport?: () => void
  onTogglePushNotifications?: () => void
  onUpdatePlan?: (planId: PlanId) => Promise<void>
  enforcePlanLimits?: boolean
  showPlanActions?: boolean
}

export default function RelayDetailCard(props: RelayDetailCardProps) {
  const r = () => props.relay
  const [planId, setPlanId] = createSignal<PlanId>(props.relay.plan_id)
  const [pendingAction, setPendingAction] = createSignal<"deactivate" | "reactivate" | null>(null)

  // Resolve the owning tenant's profile so the Tenant field can show a name and
  // avatar instead of a raw pubkey. Only relevant in admin (showTenant) views.
  // This subscription stays in the parent so the header doesn't double-subscribe.
  const tenantProfile = useProfileMetadata(() => (props.showTenant ? props.relay.tenant_pubkey : undefined))

  const memberLimitLabel = () => {
    const p = plans().find(p => p.id === r().plan_id)
    if (!p) return "?"
    return p.members === null ? "∞" : String(p.members)
  }
  const planLimited = () => (props.enforcePlanLimits ?? true) && r().plan_id === "free"
  const showPlanActions = () => props.showPlanActions ?? true
  const actionBusy = () => pendingAction() === "deactivate" ? !!props.deactivating : pendingAction() === "reactivate" ? !!props.reactivating : false
  const relayLabel = () => r().info_name || r().subdomain
  const confirmTitle = () => pendingAction() === "deactivate" ? "Deactivate relay?" : "Reactivate relay?"
  const confirmDescription = () => pendingAction() === "deactivate"
    ? `${relayLabel()} will be taken offline immediately.`
    : `${relayLabel()} will come back online and start accepting connections.`
  const confirmDetails = () => pendingAction() === "deactivate"
    ? [
        "All client connections will be dropped immediately.",
        "Members will be unable to read from or publish to the relay.",
        "Scheduled and automated tasks (billing, syncing) will be paused.",
        "All relay data, settings, and members are preserved, nothing is deleted.",
        "You can reactivate at any time from this page.",
      ]
    : undefined
  const confirmLabel = () => pendingAction() === "deactivate" ? "Yes, deactivate" : "Yes, reactivate"
  const confirmBusyLabel = () => pendingAction() === "deactivate" ? "Deactivating..." : "Reactivating..."
  const confirmTone = () => pendingAction() === "deactivate" ? "danger" : "primary"

  async function changePlanId(planId: PlanId) {
    setPlanId(planId)
    try {
      await props.onUpdatePlan?.(planId)
      setToastMessage(`Plan updated to ${planId}`, "success")
    } catch {
      // error is handled by the caller
    }
  }

  function openActionDialog(action: "deactivate" | "reactivate") {
    setPendingAction(action)
  }

  function closeActionDialog() {
    if (actionBusy()) return
    setPendingAction(null)
  }

  async function confirmAction() {
    const action = pendingAction()
    if (!action) return

    if (action === "deactivate") {
      await props.onDeactivate?.()
    } else {
      await props.onReactivate?.()
    }

    setPendingAction(null)
  }

  return (
    <div class="space-y-6">
      <RelayCardHeader
        relay={r}
        showTenant={props.showTenant}
        tenantProfile={tenantProfile}
        editHref={props.editHref}
        deactivating={props.deactivating}
        reactivating={props.reactivating}
        onRequestDeactivate={props.onDeactivate ? () => openActionDialog("deactivate") : undefined}
        onRequestReactivate={props.onReactivate ? () => openActionDialog("reactivate") : undefined}
      />

      <hr class="border-gray-200" />

      <DetailSection title="Policy">
        <ToggleField label="Public join">
          <ToggleButton
            enabled={flagToBool(r().policy_public_join, false)}
            onToggle={props.onTogglePublicJoin}
          />
        </ToggleField>
        <ToggleField label="Strip signatures">
          <ToggleButton
            enabled={flagToBool(r().policy_strip_signatures, false)}
            onToggle={props.onToggleStripSignatures}
          />
        </ToggleField>
      </DetailSection>

      <hr class="border-gray-200" />

      <DetailSection title="Features">
        <ToggleField label="Rooms">
          <ToggleButton
            enabled={flagToBool(r().groups_enabled, true)}
            onToggle={props.onToggleGroups}
          />
        </ToggleField>
        <ToggleField label="Management API">
          <ToggleButton
            enabled={flagToBool(r().management_enabled, true)}
            onToggle={props.onToggleManagement}
          />
        </ToggleField>
        <ToggleField label="Push notifications">
          <ToggleButton
            enabled={flagToBool(r().push_enabled, true)}
            onToggle={props.onTogglePushNotifications}
          />
        </ToggleField>
        <ToggleField label="Media storage">
          <PlanGatedToggle
            enabled={flagToBool(r().blossom_enabled, true)}
            fallbackEnabled={flagToBool(r().blossom_enabled, false)}
            planLimited={planLimited()}
            showPlanActions={showPlanActions()}
            canUpdatePlan={!!props.onUpdatePlan}
            editHref={props.editHref}
            onToggle={props.onToggleMediaStorage}
          />
        </ToggleField>
        <ToggleField label="LiveKit support">
          <PlanGatedToggle
            enabled={flagToBool(r().livekit_enabled, true)}
            fallbackEnabled={flagToBool(r().livekit_enabled, false)}
            planLimited={planLimited()}
            showPlanActions={showPlanActions()}
            canUpdatePlan={!!props.onUpdatePlan}
            editHref={props.editHref}
            onToggle={props.onToggleLivekitSupport}
          />
        </ToggleField>
      </DetailSection>

      <hr class="border-gray-200" />

      <MembershipSection title="Membership">
        <Field label="Current members">
          <span class="font-medium text-gray-900">{props.currentMembers ?? "—"}</span>
        </Field>
        <Field label="Member limit">
          <span class="text-gray-900">{memberLimitLabel()}</span>
        </Field>
      </MembershipSection>

      <Show when={showPlanActions()}>
        <hr class="border-gray-200" />

        <DetailSection title="Plan">
          <Show
            when={props.onUpdatePlan}
            fallback={
              <Field label="Current plan">
                <span class="capitalize text-gray-900">{r().plan_id}</span>
              </Field>
            }
          >
            <div class="lg:col-span-2 space-y-4">
              <PricingTable
                selectable
                selectedPlanId={planId()}
                onSelect={changePlanId}
              />
            </div>
          </Show>
        </DetailSection>
      </Show>

      <ConfirmDialog
        open={pendingAction() !== null}
        title={confirmTitle()}
        description={confirmDescription()}
        details={confirmDetails()}
        confirmLabel={confirmLabel()}
        busyLabel={confirmBusyLabel()}
        busy={actionBusy()}
        tone={confirmTone()}
        onConfirm={confirmAction}
        onClose={closeActionDialog}
      />
    </div>
  )
}
