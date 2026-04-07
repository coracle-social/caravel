import { A } from "@solidjs/router"
import { Show, createEffect, createSignal, onCleanup } from "solid-js"
import type { Relay, PlanId } from "@/lib/api"
import menuDotsIcon from "@/assets/menu-dots-2.svg"
import Field from "@/components/Field"
import PricingTable from "@/components/PricingTable"
import ToggleButton from "@/components/ToggleButton"
import ToggleField from "@/components/ToggleField"
import { setToastMessage } from "@/components/Toast"
import { plans } from "@/lib/state"

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
  onDeactivate?: () => void
  onReactivate?: () => void
  deactivating?: boolean
  reactivating?: boolean
  onTogglePublicJoin?: () => void
  onToggleStripSignatures?: () => void
  onToggleGroups?: () => void
  onToggleManagement?: () => void
  onToggleMediaStorage?: () => void
  onToggleLivekitSupport?: () => void
  onTogglePushNotifications?: () => void
  onUpdatePlan?: (plan: PlanId) => Promise<void>
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
  const [plan, setPlan] = createSignal<PlanId>(props.relay.plan)

  let menuContainerRef: HTMLDivElement | undefined

  const memberLimitLabel = () => {
    const p = plans().find(p => p.id === r().plan)
    if (!p) return "?"
    return p.members === null ? "∞" : String(p.members)
  }
  const planLimited = () => (props.enforcePlanLimits ?? true) && r().plan === "free"
  const showPlanActions = () => props.showPlanActions ?? true

  async function changePlan(plan: PlanId) {
    setPlan(plan)
    try {
      await props.onUpdatePlan?.(plan)
      setToastMessage(`Plan updated to ${plan}`, "success")
    } catch {
      // error is handled by the caller
    }
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
            <img src={r().info_icon} alt="" class="w-14 h-14 rounded-xl object-cover flex-shrink-0 border border-gray-200" />
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
            <Show when={r().info_description.trim()}>
              <p class="mt-2 text-sm text-gray-600">{r().info_description}</p>
            </Show>
          </div>
        </div>

        <Show when={props.editHref && (props.onDeactivate || props.onReactivate)}>
          <div class="relative flex-shrink-0" ref={menuContainerRef}>
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
                    setMenuOpen(false)
                    props.onDeactivate?.()
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
                    setMenuOpen(false)
                    props.onReactivate?.()
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
        <Show when={props.showTenant}>
          <Field label="Tenant">
            <span class="font-mono text-xs break-all">{r().tenant}</span>
          </Field>
        </Show>
      </MembershipSection>

      <Show when={showPlanActions()}>
        <hr class="border-gray-200" />

        <DetailSection title="Plan">
          <Show
            when={props.onUpdatePlan}
            fallback={
              <Field label="Current plan">
                <span class="capitalize text-gray-900">{r().plan}</span>
              </Field>
            }
          >
            <div class="lg:col-span-2 space-y-4">
              <PricingTable
                selectable
                selectedPlan={plan()}
                onSelect={changePlan}
              />
            </div>
          </Show>
        </DetailSection>
      </Show>
    </div>
  )
}
