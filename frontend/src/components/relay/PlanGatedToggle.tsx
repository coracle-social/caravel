import { A } from "@solidjs/router"
import { Show } from "solid-js"
import ToggleButton from "@/components/ToggleButton"

// Pure presentational toggle that gates a feature behind plan limits. Props are
// reactive only when read lazily, so access props.* inside JSX/derivations and
// never destructure at the top.
type PlanGatedToggleProps = {
  enabled: boolean
  fallbackEnabled: boolean
  planLimited: boolean
  showPlanActions: boolean
  canUpdatePlan: boolean
  editHref?: string
  onToggle?: () => void
}

export default function PlanGatedToggle(props: PlanGatedToggleProps) {
  return (
    <Show
      when={!props.planLimited}
      fallback={
        <Show when={props.showPlanActions} fallback={<ToggleButton enabled={props.fallbackEnabled} onToggle={props.onToggle} />}>
          <Show
            when={props.canUpdatePlan}
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
        enabled={props.enabled}
        onToggle={props.onToggle}
      />
    </Show>
  )
}
