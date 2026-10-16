import { A } from "@solidjs/router"
import { Show } from "solid-js"
import ToggleButton from "@/shared/components/ToggleButton"

type PlanGatedToggleProps = {
  enabled: boolean
  planLimited: boolean
  showPlanActions: boolean
  canUpdatePlan: boolean
  editHref?: string
  onToggle?: () => void
}

// A feature's switch, or on a plan without the feature, a prompt to upgrade.
export default function PlanGatedToggle(props: PlanGatedToggleProps) {
  return (
    <Show
      when={!props.planLimited}
      fallback={
        <Show when={props.showPlanActions} fallback={<ToggleButton enabled={props.enabled} onToggle={props.onToggle} />}>
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
