export default function ToggleButton(props: { enabled: boolean; onToggle?: () => void }) {
  const label = () => (props.enabled ? "Enabled" : "Disabled")

  return (
    <div class="inline-flex items-center gap-2">
      <button
        type="button"
        role="switch"
        aria-checked={props.enabled}
        aria-label={label()}
        class={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${props.enabled ? "bg-blue-600" : "bg-gray-300"}`}
        onClick={props.onToggle}
      >
        <span
          class={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${props.enabled ? "translate-x-5" : "translate-x-0.5"}`}
        />
      </button>
      <span class={`text-xs font-medium ${props.enabled ? "text-blue-700" : "text-gray-500"}`}>{label()}</span>
    </div>
  )
}
