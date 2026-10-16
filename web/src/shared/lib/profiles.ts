import { createEffect, createSignal, onCleanup } from "solid-js"
import { uniq } from "@welshman/lib"
import type { ProfileContent } from "applesauce-core/helpers/profile"
import { createOutboxMap, selectOptimalRelays, setFallbackRelays } from "applesauce-core/helpers/relay-selection"
import { includeMailboxes } from "applesauce-core/observable"
import { map, of } from "rxjs"
import { eventStore, pool } from "@/shared/lib/state"

export function profileName(profile: ProfileContent | undefined) {
  return profile?.name || profile?.display_name
}

// A pubkey's profile, fetched unless `prime` is false because a list fetches it.
export function useProfileMetadata(pubkey: () => string | undefined, opts?: { prime?: boolean }) {
  const prime = opts?.prime ?? true
  const [metadata, setMetadata] = createSignal<ProfileContent | undefined>()

  createEffect(() => {
    const pk = pubkey()

    if (!pk) {
      setMetadata(undefined)
      return
    }

    const profileSub = eventStore.profile(pk).subscribe(setMetadata)
    const reqSub = prime ? primeProfiles([pk]) : undefined

    onCleanup(() => {
      profileSub.unsubscribe()
      reqSub?.unsubscribe()
    })
  })

  return metadata
}

// Several pubkeys' profiles, keyed by pubkey and fetched in one request.
export function useProfileMetadataMap(pubkeys: () => string[]) {
  const [metadata, setMetadata] = createSignal<Record<string, ProfileContent | undefined>>({})

  createEffect(() => {
    const list = pubkeys()
    if (!list.length) return

    const reqSub = primeProfiles(list)
    const profileSubs = list.map((pubkey) =>
      eventStore.profile(pubkey).subscribe((profile) => {
        setMetadata((prev) => ({ ...prev, [pubkey]: profile }))
      }),
    )

    onCleanup(() => {
      reqSub.unsubscribe()
      for (const sub of profileSubs) sub.unsubscribe()
    })
  })

  return metadata
}

// Fetch a list's profiles in one request, for its items to read without fetching.
export function usePrimeProfiles(pubkeys: () => string[]) {
  createEffect(() => {
    const list = pubkeys()
    if (!list.length) return
    const sub = primeProfiles(list)
    onCleanup(() => sub.unsubscribe())
  })
}

// Fetch profiles from each pubkey's outbox relays into the event store.
function primeProfiles(pubkeys: string[]) {
  const uniquePubkeys = uniq(pubkeys.filter(Boolean))
  if (uniquePubkeys.length === 0) {
    return { unsubscribe() {} }
  }

  const seedRelays = Array.from(pool.relays.keys())
  const mailboxSeedSub = seedRelays.length
    ? pool.request(seedRelays, { kinds: [10002], authors: uniquePubkeys }).subscribe((event) => {
      eventStore.add(event)
    })
    : undefined

  const outboxMap$ = of(uniquePubkeys.map((pubkey) => ({ pubkey }))).pipe(
    includeMailboxes(eventStore),
    map((pointers) => (seedRelays.length > 0 ? setFallbackRelays(pointers, seedRelays) : pointers)),
    map((pointers) => selectOptimalRelays(pointers, { maxConnections: 8, maxRelaysPerUser: 3 })),
    map(createOutboxMap),
  )

  const profileSub = pool.outboxSubscription(outboxMap$, { kinds: [0] }).subscribe((message) => {
    if (message !== "EOSE") eventStore.add(message)
  })

  return {
    unsubscribe() {
      profileSub.unsubscribe()
      mailboxSeedSub?.unsubscribe()
    },
  }
}
