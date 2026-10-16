import { For, Show, createSignal } from "solid-js"
import { canonicalHost, relayName, type PlanId, type Relay, type RelayFlag } from "@/shared/lib/api"
import ConfirmDialog, { type Confirmation } from "@/shared/components/ConfirmDialog"
import CustomDomainModal from "@/shared/components/relay/CustomDomainModal"
import DataTransferModal from "@/shared/components/relay/DataTransferModal"
import Field from "@/shared/components/Field"
import MemberPayments from "@/shared/components/relay/MemberPayments"
import PlanGatedToggle from "@/shared/components/relay/PlanGatedToggle"
import PricingTable from "@/shared/components/PricingTable"
import RelayCardHeader from "@/shared/components/relay/RelayCardHeader"
import StatusBadge from "@/shared/components/StatusBadge"
import ToggleButton from "@/shared/components/ToggleButton"
import ToggleField from "@/shared/components/ToggleField"
import { plans, setToastMessage } from "@/shared/lib/state"
import { useProfileMetadata } from "@/shared/lib/profiles"
import useMinLoading from "@/shared/lib/useMinLoading"
import { FREE } from "@/shared/lib/plans"
import useCustomDomain from "@/shared/lib/useCustomDomain"

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

// A switch, which on a free plan asks to upgrade if `planGated`, shown while `requires` is on.
type Toggle = {
  flag: RelayFlag
  label: string
  planGated?: boolean
  requires?: RelayFlag
}

const POLICY_TOGGLES: Toggle[] = [
  { flag: "policy_public_read", label: "Public read" },
  { flag: "policy_public_write", label: "Public write" },
  { flag: "policy_public_join", label: "Public join" },
  { flag: "members_can_invite", label: "Members can invite others" },
  { flag: "policy_strip_signatures", label: "Strip signatures" },
]

const FEATURE_TOGGLES: Toggle[] = [
  { flag: "groups_enabled", label: "Rooms" },
  { flag: "management_enabled", label: "Management API" },
  { flag: "push_enabled", label: "Push notifications" },
  { flag: "blossom_enabled", label: "Media storage", planGated: true },
  { flag: "blossom_authenticated_read", label: "Members-only media", requires: "blossom_enabled" },
  { flag: "livekit_enabled", label: "LiveKit support", planGated: true },
]

type Action = "deactivate" | "reactivate" | "purge"

function confirmation(action: Action, label: string): Confirmation {
  switch (action) {
    case "purge":
      return {
        title: "Purge relay?",
        description: `${label} and all of its data will be permanently deleted.`,
        details: [
          "All events, uploads, members, and settings are deleted from the hosting backend.",
          "The relay's subdomain and custom domain stop serving immediately.",
          "An active paid relay earns a prorated credit for the rest of the billing period.",
          "This cannot be undone — a purged relay cannot be re-activated.",
        ],
        confirmLabel: "Yes, purge relay",
        busyLabel: "Purging...",
        tone: "danger",
      }
    case "deactivate":
      return {
        title: "Deactivate relay?",
        description: `${label} will be taken offline immediately.`,
        details: [
          "All client connections will be dropped immediately.",
          "Members will be unable to read from or publish to the relay.",
          "Scheduled and automated tasks (billing, syncing) will be paused.",
          "All relay data, settings, and members are preserved, nothing is deleted.",
          "You can reactivate at any time from this page.",
        ],
        confirmLabel: "Yes, deactivate",
        busyLabel: "Deactivating...",
        tone: "danger",
      }
    case "reactivate":
      return {
        title: "Reactivate relay?",
        description: `${label} will come back online and start accepting connections.`,
        confirmLabel: "Yes, reactivate",
        busyLabel: "Reactivating...",
        tone: "primary",
      }
  }
}

type RelayDetailCardProps = {
  relay: Relay
  currentMembers?: number
  showTenant?: boolean
  editHref?: string
  busy?: boolean
  onDeactivate?: () => void | Promise<void>
  onReactivate?: () => void | Promise<void>
  onPurge?: () => void | Promise<void>
  onToggle?: (flag: RelayFlag) => void
  onUpdatePlan?: (planId: PlanId) => Promise<void>
  enforcePlanLimits?: boolean
  showPlanActions?: boolean
  mutateRelay?: (relay: Relay) => void
}

export default function RelayDetailCard(props: RelayDetailCardProps) {
  const r = () => props.relay
  const [planId, setPlanId] = createSignal<PlanId>(props.relay.plan_id)
  const [pendingAction, setPendingAction] = createSignal<Action>()
  const [customDomainModalOpen, setCustomDomainModalOpen] = createSignal(false)
  const [dataTransferModalOpen, setDataTransferModalOpen] = createSignal(false)
  const { saving: cdSaving, verifying: cdVerifying, error: cdError, saveDomain, verifyDomain } =
    useCustomDomain(() => props.relay.id, props.mutateRelay ?? (() => {}))
  const cdVerifyingVisible = useMinLoading(cdVerifying)

  // Only admin views show the tenant, and the header reads it from here.
  const tenantProfile = useProfileMetadata(() => (props.showTenant ? props.relay.tenant_pubkey : undefined))

  const memberLimitLabel = () => {
    const p = plans().find(p => p.id === r().plan_id)
    if (!p) return "?"
    return p.members === null ? "∞" : String(p.members)
  }
  const planLimited = () => (props.enforcePlanLimits ?? true) && r().plan_id === FREE
  const showPlanActions = () => props.showPlanActions ?? true
  const busy = () => props.busy ?? false

  async function changePlanId(planId: PlanId) {
    setPlanId(planId)
    try {
      await props.onUpdatePlan?.(planId)
      setToastMessage(`Plan updated to ${planId}`, "success")
    } catch {
      // The caller reports the error.
    }
  }

  function closeActionDialog() {
    if (busy()) return
    setPendingAction(undefined)
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

    setPendingAction(undefined)
  }

  const renderToggle = (toggle: Toggle) => (
    <Show when={!toggle.requires || r()[toggle.requires] === 1}>
      <ToggleField label={toggle.label}>
        <Show
          when={toggle.planGated}
          fallback={<ToggleButton enabled={r()[toggle.flag] === 1} onToggle={() => props.onToggle?.(toggle.flag)} />}
        >
          <PlanGatedToggle
            enabled={r()[toggle.flag] === 1}
            planLimited={planLimited()}
            showPlanActions={showPlanActions()}
            canUpdatePlan={!!props.onUpdatePlan}
            editHref={props.editHref}
            onToggle={() => props.onToggle?.(toggle.flag)}
          />
        </Show>
      </ToggleField>
    </Show>
  )

  return (
    <div class="space-y-6">
      <RelayCardHeader
        relay={r}
        showTenant={props.showTenant}
        tenantProfile={tenantProfile}
        editHref={props.editHref}
        busy={busy()}
        onRequestDeactivate={props.onDeactivate ? () => setPendingAction("deactivate") : undefined}
        onRequestReactivate={props.onReactivate ? () => setPendingAction("reactivate") : undefined}
        onRequestManageCustomDomain={() => setCustomDomainModalOpen(true)}
        onRequestTransferData={() => setDataTransferModalOpen(true)}
        onRequestPurge={props.onPurge ? () => setPendingAction("purge") : undefined}
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
                {r().custom_domain} CNAME {canonicalHost(r())}
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

      <DataTransferModal
        open={dataTransferModalOpen()}
        onClose={() => setDataTransferModalOpen(false)}
        relay={r}
      />

      <hr class="border-gray-200" />

      <DetailSection title="Policy">
        <For each={POLICY_TOGGLES}>{renderToggle}</For>
      </DetailSection>

      <hr class="border-gray-200" />

      <DetailSection title="Features">
        <For each={FEATURE_TOGGLES}>{renderToggle}</For>
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

      <Show when={props.mutateRelay}>
        {(mutate) => (
          <>
            <hr class="border-gray-200" />
            <MemberPayments relay={r} mutateRelay={mutate()} />
          </>
        )}
      </Show>

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
        confirmation={pendingAction() && confirmation(pendingAction()!, relayName(r()))}
        busy={busy()}
        onConfirm={confirmAction}
        onClose={closeActionDialog}
      />
    </div>
  )
}
