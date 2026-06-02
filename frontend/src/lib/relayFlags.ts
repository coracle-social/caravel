// Relay feature/policy flags are stored on the wire as numeric 0/1 (see
// Relay/CreateRelayInput/UpdateRelayInput in api.ts). These helpers centralize
// the boolean<->0/1 conversion so it isn't duplicated across the toggle UI and
// the toggle mutations. The wire shape stays numeric: boolToFlag returns the
// literal union `0 | 1` so it remains assignable to the `number` input fields.

export function flagToBool(value: number | undefined, fallback: boolean): boolean {
  if (value === 0) return false
  if (value === 1) return true
  return fallback
}

export function boolToFlag(value: boolean): 0 | 1 {
  return value ? 1 : 0
}
