import { createComponent, createContext, useContext } from "solid-js"
import type { JSX } from "solid-js"
import type { IAccount } from "applesauce-accounts"
import type { EventStore } from "applesauce-core"
import type { RelayPool } from "applesauce-relay"
import { account, eventStore, pool } from "@/shared/lib/state"

// A single lightweight DI seam for the app's nostr singletons. eventStore and
// pool are genuine app-lifetime singletons; account is a reactive signal. This
// context lets call sites reach them without importing the module globals
// directly, while falling back to the live singletons when no Provider is
// mounted — so production wiring stays byte-for-byte identical and the seam
// only matters for tests/alternate mounts.
export type NostrContext = {
  account: () => IAccount | undefined
  eventStore: EventStore
  pool: RelayPool
}

const NostrContextImpl = createContext<NostrContext>()

export function useNostr(): NostrContext {
  return useContext(NostrContextImpl) ?? { account, eventStore, pool }
}

// Authored without JSX so this stays a plain .ts module. createComponent renders
// the context Provider with the given value, wrapping the children.
export function NostrProvider(props: { value: NostrContext; children?: JSX.Element }): JSX.Element {
  return createComponent(NostrContextImpl.Provider, {
    value: props.value,
    get children() {
      return props.children
    },
  })
}
