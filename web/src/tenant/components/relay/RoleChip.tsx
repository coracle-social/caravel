import type { RelayRole } from "@/shared/lib/api"

// A role's label in its color, or the bare id of a role the relay no longer defines.
export default function RoleChip(props: { id: string; roles: RelayRole[] }) {
  const role = () => props.roles.find(role => role.id === props.id)

  return (
    <span class="inline-flex items-center gap-1.5 rounded-full border border-gray-200 px-2 py-0.5 text-xs text-gray-700">
      <span class="h-2 w-2 rounded-full" style={{ background: `hsl(${role()?.hue ?? 0} 65% 50%)` }} />
      {role()?.label || props.id}
    </span>
  )
}
