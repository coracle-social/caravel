import { A } from "@solidjs/router"
import { Show, createEffect, createSignal, onCleanup } from "solid-js"
import type { Relay } from "../lib/api"
import menuDotsIcon from "../assets/menu-dots-2.svg"
import Modal from "./Modal"
import PricingTable from "./PricingTable"
import { RELAY_PLAN_IDS, type RelayPlanId } from "../lib/relayPlans"

function Field(props: { label: string; children: any }) {
  return (
    <div>
      <dt class="text-xs font-medium text-gray-500 uppercase tracking-wide">{props.label}</dt>
      <dd class="mt-0.5 text-sm text-gray-900">{props.children}</dd>
    </div>
  )
}

function ToggleField(props: { label: string; children: any }) {
  return (
    <div class="flex items-center justify-between gap-3">
      <dt class="text-xs font-medium text-gray-500 uppercase tracking-wide">{props.label}</dt>
      <dd class="text-sm text-gray-900">{props.children}</dd>
    </div>
  )
}

function ToggleButton(props: {
  enabled: boolean
  disabled?: boolean
  onToggle?: () => void
  onLabel?: string
  offLabel?: string
}) {
  const label = () => (props.enabled ? (props.onLabel ?? "Enabled") : (props.offLabel ?? "Disabled"))

  return (
    <div class="inline-flex items-center gap-2">
      <button
        type="button"
        role="switch"
        aria-checked={props.enabled}
        aria-label={label()}
        class={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${props.enabled ? "bg-blue-600" : "bg-gray-300"} disabled:opacity-50`}
        onClick={props.onToggle}
        disabled={props.disabled}
      >
        <span
          class={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${props.enabled ? "translate-x-5" : "translate-x-0.5"}`}
        />
      </button>
      <span class={`text-xs font-medium ${props.enabled ? "text-blue-700" : "text-gray-500"}`}>{label()}</span>
    </div>
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
  deactivating?: boolean
  onTogglePublicJoin?: () => void
  onToggleStripSignatures?: () => void
  onToggleGroups?: () => void
  onToggleManagement?: () => void
  onToggleMediaStorage?: () => void
  onToggleLivekitSupport?: () => void
  onTogglePushNotifications?: () => void
  onUpdatePlan?: (plan: RelayPlanId) => Promise<void>
  updatingPlan?: boolean
  enforcePlanLimits?: boolean
  showPlanActions?: boolean
}

export default function RelayDetailCard(props: RelayDetailCardProps) {
  const r = () => props.relay
  const cfg = () => r().config
  const [menuOpen, setMenuOpen] = createSignal(false)
  const [planModalOpen, setPlanModalOpen] = createSignal(false)
  const [selectedPlan, setSelectedPlan] = createSignal<RelayPlanId>("free")
  const [planError, setPlanError] = createSignal("")
  const [submittingPlan, setSubmittingPlan] = createSignal(false)
  let menuContainerRef: HTMLDivElement | undefined

  const memberLimitByPlan: Record<string, string> = {
    free: "10",
    basic: "100",
    growth: "∞",
  }

  const memberLimitLabel = () => memberLimitByPlan[r().plan] ?? "?"
  const isTopTier = () => r().plan === "growth"
  const planLimited = () => (props.enforcePlanLimits ?? true) && r().plan === "free"
  const showPlanActions = () => props.showPlanActions ?? true

  createEffect(() => {
    if (!planModalOpen()) return
    const current = RELAY_PLAN_IDS.find((id) => id === r().plan) ?? "free"
    setSelectedPlan(current)
    setPlanError("")
  })

  const canSubmitPlan = () => selectedPlan() !== r().plan

  async function handlePlanContinue() {
    if (!props.onUpdatePlan || submittingPlan() || !canSubmitPlan()) return
    setPlanError("")
    setSubmittingPlan(true)

    try {
      await props.onUpdatePlan(selectedPlan())
      setPlanModalOpen(false)
    } catch (e) {
      setPlanError(e instanceof Error ? e.message : "Failed to update plan")
    } finally {
      setSubmittingPlan(false)
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
          <Show when={r().icon}>
            <img src={r().icon} alt="" class="w-14 h-14 rounded-xl object-cover flex-shrink-0 border border-gray-200" />
          </Show>
          <div class="min-w-0">
            <h1 class="text-2xl font-bold text-gray-900">{r().name}</h1>
            <a
              href={`wss://${r().subdomain}.spaces.coracle.social`}
              class="text-sm text-blue-600 hover:underline break-all"
            >
              wss://{r().subdomain}.spaces.coracle.social
            </a>
            <Show when={r().description.trim()}>
              <p class="mt-2 text-sm text-gray-600">{r().description}</p>
            </Show>
          </div>
        </div>

        <Show when={props.editHref && props.onDeactivate}>
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
            </div>
          </div>
        </Show>
      </div>

      <hr class="border-gray-200" />

      <DetailSection title="Policy">
        <ToggleField label="Public join">
          <ToggleButton
            enabled={cfg()?.policy.public_join ?? false}
            onToggle={props.onTogglePublicJoin}
          />
        </ToggleField>
        <ToggleField label="Strip signatures">
          <ToggleButton
            enabled={cfg()?.policy.strip_signatures ?? false}
            onToggle={props.onToggleStripSignatures}
          />
        </ToggleField>
      </DetailSection>

      <hr class="border-gray-200" />

      <DetailSection title="Features">
        <ToggleField label="Rooms">
          <ToggleButton
            enabled={cfg()?.groups.enabled ?? true}
            onToggle={props.onToggleGroups}
          />
        </ToggleField>
        <ToggleField label="Management API">
          <ToggleButton
            enabled={cfg()?.management.enabled ?? true}
            onToggle={props.onToggleManagement}
          />
        </ToggleField>
        <ToggleField label="Push notifications">
          <ToggleButton
            enabled={cfg()?.push.enabled ?? true}
            onToggle={props.onTogglePushNotifications}
          />
        </ToggleField>
        <ToggleField label="Media storage">
          <Show
            when={!planLimited()}
            fallback={
              <Show when={showPlanActions()} fallback={<ToggleButton enabled={cfg()?.blossom.enabled ?? false} onToggle={props.onToggleMediaStorage} />}>
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
                  <button
                    type="button"
                    class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                    onClick={() => setPlanModalOpen(true)}
                  >
                    Update Plan
                  </button>
                </Show>
              </Show>
            }
          >
            <ToggleButton
              enabled={cfg()?.blossom.enabled ?? true}
              onToggle={props.onToggleMediaStorage}
            />
          </Show>
        </ToggleField>
        <ToggleField label="LiveKit support">
          <Show
            when={!planLimited()}
            fallback={
              <Show when={showPlanActions()} fallback={<ToggleButton enabled={cfg()?.livekit.enabled ?? false} onToggle={props.onToggleLivekitSupport} />}>
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
                  <button
                    type="button"
                    class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                    onClick={() => setPlanModalOpen(true)}
                  >
                    Update Plan
                  </button>
                </Show>
              </Show>
            }
          >
            <ToggleButton
              enabled={cfg()?.livekit.enabled ?? true}
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
        <Show when={props.editHref && showPlanActions()}>
          <Field label=" ">
            <Show
              when={props.onUpdatePlan}
              fallback={
                <Show
                  when={!isTopTier()}
                  fallback={<span />}
                >
                  <A
                    href={props.editHref!}
                    class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                  >
                    Upgrade Plan
                  </A>
                </Show>
              }
            >
              <button
                type="button"
                class="inline-flex items-center rounded-lg border border-blue-200 bg-blue-50 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-100"
                onClick={() => setPlanModalOpen(true)}
              >
                Update Plan
              </button>
            </Show>
          </Field>
        </Show>
      </MembershipSection>

      <Modal
        open={planModalOpen()}
        onClose={() => setPlanModalOpen(false)}
        wrapperClass="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
        panelClass="w-full max-w-5xl max-h-[90vh] overflow-y-auto rounded-2xl bg-white"
      >
        <div class="p-6 sm:p-8">
          <h2 class="text-2xl font-bold text-gray-900 mb-2">Update plan</h2>
          <p class="text-sm text-gray-500 mb-8">Choose the plan you want for this relay.</p>

          <PricingTable
            selectable
            compactOnMobile
            selectedPlan={selectedPlan()}
            onSelect={setSelectedPlan}
          />

          <Show when={planError()}>
            <p class="mt-4 text-sm text-red-600">{planError()}</p>
          </Show>

          <div class="mt-8">
            <button
              type="button"
              onClick={() => void handlePlanContinue()}
              disabled={!canSubmitPlan() || submittingPlan() || !!props.updatingPlan}
              class="w-full py-3 rounded-xl bg-blue-600 text-white font-semibold hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {submittingPlan() || props.updatingPlan ? "Updating..." : "Continue"}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
