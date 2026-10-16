import { useSearchParams } from "@solidjs/router"

// One value of the page's query string.
export function useParam() {
  const [search, setSearch] = useSearchParams()
  const param = (name: string) => {
    const value = search[name]
    return Array.isArray(value) ? value[0] : value
  }
  return [param, setSearch] as const
}

// The client a member came from, if it is an https link to offer back to them.
export function returnLink(value: string | undefined) {
  try {
    const url = new URL(value ?? "")
    return url.protocol === "https:" ? url : undefined
  } catch {
    return undefined
  }
}
