export default function Checkbox(props: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label class="flex items-center gap-2 cursor-pointer">
      <input
        type="checkbox"
        checked={props.checked}
        onChange={e => props.onChange(e.currentTarget.checked)}
        class="w-4 h-4 rounded border-gray-300 text-blue-600"
      />
      <span class="text-sm text-gray-700">{props.label}</span>
    </label>
  )
}
