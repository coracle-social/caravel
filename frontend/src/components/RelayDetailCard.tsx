import { Show, createSignal } from "solid-js"
import type { Relay, PlanId } from "@/lib/api"
import ConfirmDialog from "@/components/ConfirmDialog"
import CustomDomainModal from "@/components/relay/CustomDomainModal"
import Field from "@/components/Field"
import PricingTable from "@/components/PricingTable"
import ToggleButton from "@/components/ToggleButton"
import ToggleField from "@/components/ToggleField"
import RelayCardHeader, { StatusBadge } from "@/components/relay/RelayCardHeader"
import PlanGatedToggle from "@/components/relay/PlanGatedToggle"
import { setToastMessage } from "@/lib/state"
import { useProfileMetadata } from "@/lib/hooks"
import useMinLoading from "@/lib/useMinLoading"
import { flagToBool } from "@/lib/relayFlags"
import { plans } from "@/lib/state"
import useCustomDomain from "@/lib/useCustomDomain"

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
  onPurge?: () => void | Promise<void>
  purging?: boolean
  onTogglePublicRead?: () => void
  onTogglePublicWrite?: () => void
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
  mutateRelay?: (relay: Relay) => void
}

export default function RelayDetailCard(props: RelayDetailCardProps) {
  const r = () => props.relay
  const [planId, setPlanId] = createSignal<PlanId>(props.relay.plan_id)
  const [pendingAction, setPendingAction] = createSignal<"deactivate" | "reactivate" | "purge" | null>(null)
  const [customDomainModalOpen, setCustomDomainModalOpen] = createSignal(false)
  const { saving: cdSaving, verifying: cdVerifying, error: cdError, saveDomain, verifyDomain } =
    useCustomDomain(() => props.relay.id, props.mutateRelay ?? (() => {}))
  const cdVerifyingVisible = useMinLoading(cdVerifying)

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
  const actionBusy = () => pendingAction() === "deactivate" ? !!props.deactivating : pendingAction() === "reactivate" ? !!props.reactivating : pendingAction() === "purge" ? !!props.purging : false
  const relayLabel = () => r().info_name || r().subdomain
  const confirmTitle = () => pendingAction() === "purge" ? "Purge relay?" : pendingAction() === "deactivate" ? "Deactivate relay?" : "Reactivate relay?"
  const confirmDescription = () => pendingAction() === "purge"
    ? `${relayLabel()} and all of its data will be permanently deleted.`
    : pendingAction() === "deactivate"
      ? `${relayLabel()} will be taken offline immediately.`
      : `${relayLabel()} will come back online and start accepting connections.`
  const confirmDetails = () => pendingAction() === "purge"
    ? [
        "All events, uploads, members, and settings are deleted from the hosting backend.",
        "The relay's subdomain and custom domain stop serving immediately.",
        "An active paid relay earns a prorated credit for the rest of the billing period.",
        "This cannot be undone — a purged relay cannot be re-activated.",
      ]
    : pendingAction() === "deactivate"
      ? [
          "All client connections will be dropped immediately.",
          "Members will be unable to read from or publish to the relay.",
          "Scheduled and automated tasks (billing, syncing) will be paused.",
          "All relay data, settings, and members are preserved, nothing is deleted.",
          "You can reactivate at any time from this page.",
        ]
      : undefined
  const confirmLabel = () => pendingAction() === "purge" ? "Yes, purge relay" : pendingAction() === "deactivate" ? "Yes, deactivate" : "Yes, reactivate"
  const confirmBusyLabel = () => pendingAction() === "purge" ? "Purging..." : pendingAction() === "deactivate" ? "Deactivating..." : "Reactivating..."
  const confirmTone = () => pendingAction() === "reactivate" ? "primary" : "danger"

  async function changePlanId(planId: PlanId) {
    setPlanId(planId)
    try {
      await props.onUpdatePlan?.(planId)
      setToastMessage(`Plan updated to ${planId}`, "success")
    } catch {
      // error is handled by the caller
    }
  }

  function openActionDialog(action: "deactivate" | "reactivate" | "purge") {
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
    } else if (action === "reactivate") {
      await props.onReactivate?.()
    } else {
      await props.onPurge?.()
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
        purging={props.purging}
        onRequestDeactivate={props.onDeactivate ? () => openActionDialog("deactivate") : undefined}
        onRequestReactivate={props.onReactivate ? () => openActionDialog("reactivate") : undefined}
        onRequestManageCustomDomain={() => setCustomDomainModalOpen(true)}
        onRequestPurge={props.onPurge ? () => openActionDialog("purge") : undefined}
      />

      <hr class="border-gray-200" />

      <div>
        <h3 class="text-sm font-semibold uppercase tracking-wider mb-6">Custom Domain</h3>
        <div class="space-y-3">
          <div class="flex items-center justify-between gap-2">
            <Show
              when={r().custom_domain}
              fallback={<span class="text-gray-400 text-sm">Not configured</span>}
            >
              <div class="flex items-center gap-2">
                <span class="font-medium text-gray-900">{r().custom_domain}</span>
                <Show when={r().custom_domain_verified === 1}>
                  <StatusBadge status="verified" />
                </Show>
              </div>
            </Show>
            <button
              class="inline-flex items-center rounded-lg border border-gray-200 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
              onClick={() => setCustomDomainModalOpen(true)}
            >
              Update
            </button>
          </div>
          <Show when={r().custom_domain && r().custom_domain_verified !== 1}>
            <div class="rounded-lg border border-yellow-200 bg-yellow-50 px-4 py-3 space-y-3">
              <div class="flex items-start gap-2">
                <span class="mt-0.5 text-yellow-600 text-sm shrink-0">⚠</span>
                <p class="text-sm font-medium text-yellow-800">
                  Not yet verified — add this DNS record, then verify:
                </p>
              </div>
              <div class="rounded border border-yellow-200 bg-white px-3 py-2 font-mono text-xs text-gray-700 break-all">
                {r().custom_domain} CNAME {r().subdomain}.{r().zooid_domain}
              </div>
              <p class="text-xs text-yellow-700">
                For apex domains (e.g. example.com), use an ALIAS or ANAME record instead.
              </p>
              <Show when={cdError()}>
                <p class="text-sm text-red-600">{cdError()}</p>
              </Show>
              <button
                type="button"
                onClick={verifyDomain}
                disabled={cdVerifyingVisible()}
                class="inline-flex items-center rounded-lg bg-yellow-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-yellow-700 disabled:opacity-50"
              >
                {cdVerifyingVisible() ? "Verifying…" : "Verify DNS record"}
              </button>
            </div>
          </Show>
        </div>
      </div>

      <CustomDomainModal
        open={customDomainModalOpen()}
        onClose={() => setCustomDomainModalOpen(false)}
        relay={r}
        saving={cdSaving}
        error={cdError}
        onSave={saveDomain}
      />

      <hr class="border-gray-200" />

      <DetailSection title="Policy">
        <ToggleField label="Public read">
          <ToggleButton
            enabled={flagToBool(r().policy_public_read, false)}
            onToggle={props.onTogglePublicRead}
          />
        </ToggleField>
        <ToggleField label="Public write">
          <ToggleButton
            enabled={flagToBool(r().policy_public_write, false)}
            onToggle={props.onTogglePublicWrite}
          />
        </ToggleField>
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
