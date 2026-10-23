import { A, useLocation, useParams, type RouteSectionProps } from "@solidjs/router"
import { createContext, For, useContext } from "solid-js"
import BackLink from "@/shared/components/BackLink"
import { useRelay } from "@/shared/lib/relays"

const RelayContext = createContext<ReturnType<typeof useRelay>>()

// The relay the pages inside RelayFrame manage, loaded once for all of them.
export function useFramedRelay() {
  const relay = useContext(RelayContext)
  if (!relay) throw new Error("useFramedRelay is only for pages inside RelayFrame")
  return relay
}

const SECTIONS = [
  { path: "", label: "Overview" },
  { path: "/members", label: "Members" },
  { path: "/rooms", label: "Rooms" },
  { path: "/reports", label: "Reports" },
  { path: "/content", label: "Content" },
  { path: "/bans", label: "Bans" },
]

// A relay's pages: back to the relay list, then a tab for each part of managing the relay.
export default function RelayFrame(props: RouteSectionProps) {
  const params = useParams()
  const location = useLocation()
  const relay = useRelay(() => params.id ?? "")

  const base = () => `/relays/${params.id}`
  const sectionClass = (path: string) => {
    const active = path ? location.pathname.startsWith(base() + path) : location.pathname === base()
    return active
      ? "shrink-0 border-b-2 border-gray-900 px-3 py-2 text-sm font-medium text-gray-900"
      : "shrink-0 border-b-2 border-transparent px-3 py-2 text-sm text-gray-500 hover:text-gray-800"
  }

  return (
    <RelayContext.Provider value={relay}>
      <div class="mx-auto max-w-4xl px-4 pt-8">
        <BackLink href="/relays" label="Relays" />
        <nav class="-mt-2 flex overflow-x-auto border-b border-gray-200">
          <For each={SECTIONS}>
            {(section) => <A href={base() + section.path} class={sectionClass(section.path)}>{section.label}</A>}
          </For>
        </nav>
      </div>
      {props.children}
    </RelayContext.Provider>
  )
}
