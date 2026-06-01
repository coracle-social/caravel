import { A } from "@solidjs/router"
import { Show, createEffect, createSignal, onCleanup } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import type { Relay, PlanId } from "@/lib/api"
import menuDotsIcon from "@/assets/menu-dots-2.svg"
import ConfirmDialog from "@/components/ConfirmDialog"
import Field from "@/components/Field"
import PricingTable from "@/components/PricingTable"
import ToggleButton from "@/components/ToggleButton"
import ToggleField from "@/components/ToggleField"
import { setToastMessage } from "@/components/Toast"
import { primeProfiles } from "@/lib/hooks"
import { eventStore, plans } from "@/lib/state"

function shortenPubkey(pubkey: string) {
  return pubkey.length <= 16 ? pubkey : `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

const STATUS_STYLES: Record<string, string> = {
  active:   "bg-green-50 text-green-700 border-green-200",
  inactive: "bg-gray-100 text-gray-500 border-gray-200",
}

function StatusBadge(props: { status: string }) {
  const styles = () => STATUS_STYLES[props.status] ?? "bg-gray-100 text-gray-500 border-gray-200"
  const label = () => props.status.replace(/_/g, " ")
  return (
    <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${styles()}`}>
      {label()}
    </span>
  )
}

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
  const flag = (value: number, fallback: boolean) => {
    if (value === 0) return false
    if (value === 1) return true
    return fallback
  }
  const [menuOpen, setMenuOpen] = createSignal(false)
  const [planId, setPlanId] = createSignal<PlanId>(props.relay.plan_id)
  const [pendingAction, setPendingAction] = createSignal<"deactivate" | "reactivate" | null>(null)
  const [tenantProfile, setTenantProfile] = createSignal<{ name?: string; picture?: string }>({})

  // Resolve the owning tenant's profile so the Tenant field can show a name and
  // avatar instead of a raw pubkey. Only relevant in admin (showTenant) views.
  createEffect(() => {
    if (!props.showTenant) return
    const pubkey = props.relay.tenant_pubkey
    if (!pubkey) return

    const sub = eventStore.profile(pubkey).subscribe((metadata) => {
      setTenantProfile({
        name: metadata?.name || metadata?.display_name,
        picture: getProfilePicture(metadata),
      })
    })
    const primeSub = primeProfiles([pubkey])

    onCleanup(() => {
      sub.unsubscribe()
      primeSub.unsubscribe()
    })
  })

  let menuContainerRef: HTMLDivElement | undefined

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
    setMenuOpen(false)
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

  createEffect(() => {
    if (!menuOpen()) return

    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node | null
      if (target && !menuContainerRef?.contains(target)) {
        setMenuOpen(false)
      }
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false)
      }
    }

    document.addEventListener("mousedown", handleClickOutside)
    document.addEventListener("keydown", handleEscape)

    onCleanup(() => {
      document.removeEventListener("mousedown", handleClickOutside)
      document.removeEventListener("keydown", handleEscape)
    })
  })

  return (
    <div class="space-y-6">
      {/* Header */}
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-start gap-4 min-w-0">
          <Show when={r().info_icon}>
            <img src={r().info_icon} alt="" class="w-14 h-14 rounded-xl object-cover shrink-0 border border-gray-200" />
          </Show>
          <div class="min-w-0">
            <div class="flex items-center gap-3 flex-wrap">
              <h1 class="text-2xl font-bold text-gray-900">{r().info_name || r().subdomain}</h1>
              <StatusBadge status={r().status} />
            </div>
            <a
              href={`wss://${r().subdomain}.spaces.coracle.social`}
              class="text-sm text-blue-600 hover:underline break-all"
            >
              wss://{r().subdomain}.spaces.coracle.social
            </a>
            <Show when={props.showTenant}>
              <A href={`/admin/tenants/${r().tenant_pubkey}`} class="group mt-1.5 flex w-fit items-center gap-2 min-w-0">
                <Show
                  when={tenantProfile().picture}
                  fallback={
                    <div class="h-6 w-6 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                      {(tenantProfile().name || r().tenant_pubkey).slice(0, 1).toUpperCase()}
                    </div>
                  }
                >
                  <img src={tenantProfile().picture} alt="" class="h-6 w-6 flex-shrink-0 rounded-full object-cover" />
                </Show>
                <span class="text-sm text-blue-600 group-hover:underline truncate">{tenantProfile().name || shortenPubkey(r().tenant_pubkey)}</span>
              </A>
            </Show>
            <Show when={r().info_description.trim()}>
              <p class="mt-2 text-sm text-gray-600">{r().info_description}</p>
            </Show>
          </div>
        </div>

        <Show when={props.editHref && (props.onDeactivate || props.onReactivate)}>
          <div class="relative shrink-0" ref={menuContainerRef}>
            <button
              type="button"
              class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-gray-200 hover:bg-gray-50"
              aria-label="Open relay actions"
              onClick={() => setMenuOpen((open) => !open)}
            >
              <img src={menuDotsIcon} alt="" class="h-5 w-5" />
            </button>

            <div
              class="absolute right-0 mt-2 w-44 rounded-lg border border-gray-200 bg-white shadow-lg z-10 py-1 origin-top-right transition-all duration-150 ease-out"
              classList={{
                "opacity-100 scale-100": menuOpen(),
                "opacity-0 scale-95 pointer-events-none": !menuOpen(),
              }}
            >
              <A
                href={props.editHref!}
                class="block px-3 py-2 text-sm text-gray-700 hover:bg-gray-50"
                onClick={() => setMenuOpen(false)}
              >
                Edit Details
              </A>
              <Show when={r().status === "active" && props.onDeactivate}>
                <button
                  type="button"
                  class="block w-full text-left px-3 py-2 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50"
                  onClick={() => {
                    openActionDialog("deactivate")
                  }}
                  disabled={props.deactivating}
                >
                  {props.deactivating ? "Deactivating..." : "Deactivate"}
                </button>
              </Show>
              <Show when={r().status === "inactive" && props.onReactivate}>
                <button
                  type="button"
                  class="block w-full text-left px-3 py-2 text-sm text-blue-600 hover:bg-blue-50 disabled:opacity-50"
                  onClick={() => {
                    openActionDialog("reactivate")
                  }}
                  disabled={props.reactivating}
                >
                  {props.reactivating ? "Reactivating..." : "Reactivate"}
                </button>
              </Show>
            </div>
          </div>
        </Show>
      </div>

      <Show when={r().sync_error}>
        <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-3">
          <p class="text-sm font-semibold text-red-800">Provisioning error</p>
          <p class="mt-1 text-sm text-red-700 font-mono break-all">{r().sync_error}</p>
        </div>
      </Show>

      <hr class="border-gray-200" />

      <DetailSection title="Policy">
        <ToggleField label="Public join">
          <ToggleButton
            enabled={flag(r().policy_public_join, false)}
            onToggle={props.onTogglePublicJoin}
          />
        </ToggleField>
        <ToggleField label="Strip signatures">
          <ToggleButton
            enabled={flag(r().policy_strip_signatures, false)}
            onToggle={props.onToggleStripSignatures}
          />
        </ToggleField>
      </DetailSection>

      <hr class="border-gray-200" />

      <DetailSection title="Features">
        <ToggleField label="Rooms">
          <ToggleButton
            enabled={flag(r().groups_enabled, true)}
            onToggle={props.onToggleGroups}
          />
        </ToggleField>
        <ToggleField label="Management API">
          <ToggleButton
            enabled={flag(r().management_enabled, true)}
            onToggle={props.onToggleManagement}
          />
        </ToggleField>
        <ToggleField label="Push notifications">
          <ToggleButton
            enabled={flag(r().push_enabled, true)}
            onToggle={props.onTogglePushNotifications}
          />
        </ToggleField>
        <ToggleField label="Media storage">
          <Show
            when={!planLimited()}
            fallback={
              <Show when={showPlanActions()} fallback={<ToggleButton enabled={flag(r().blossom_enabled, false)} onToggle={props.onToggleMediaStorage} />}>
                <Show
                  when={props.onUpdatePlan}
                  fallback={
                    <A
                      href={props.editHref ?? "#"}
                      class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                    >
                      Upgrade Plan
                    </A>
                  }
                >
                  <span class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700">
                    Update Plan
                  </span>
                </Show>
              </Show>
            }
          >
            <ToggleButton
              enabled={flag(r().blossom_enabled, true)}
              onToggle={props.onToggleMediaStorage}
            />
          </Show>
        </ToggleField>
        <ToggleField label="LiveKit support">
          <Show
            when={!planLimited()}
            fallback={
              <Show when={showPlanActions()} fallback={<ToggleButton enabled={flag(r().livekit_enabled, false)} onToggle={props.onToggleLivekitSupport} />}>
                <Show
                  when={props.onUpdatePlan}
                  fallback={
                    <A
                      href={props.editHref ?? "#"}
                      class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                    >
                      Upgrade Plan
                    </A>
                  }
                >
                  <span class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700">
                    Update Plan
                  </span>
                </Show>
              </Show>
            }
          >
            <ToggleButton
              enabled={flag(r().livekit_enabled, true)}
              onToggle={props.onToggleLivekitSupport}
            />
          </Show>
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
