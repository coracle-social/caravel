import { createSignal, onMount } from "solid-js"
import { listRelayEvents, type NostrEvent } from "@/shared/lib/api"

// A relay's events of some kinds (any, if none), without those it signed itself if asked, newest first, a page at a time.
export default function usePagedEvents(relayId: () => string, query: { kinds?: number[]; exclude_own?: boolean } = {}, pageSize = 30) {
  const [events, setEvents] = createSignal<NostrEvent[]>([])
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<unknown>()
  const [exhausted, setExhausted] = createSignal(false)

  // The last event fetched, which the next page starts after.
  let cursor: NostrEvent | undefined

  async function loadMore() {
    if (loading() || exhausted()) return

    setLoading(true)
    setError(undefined)
    try {
      const page = await listRelayEvents(relayId(), { ...query, until: cursor?.created_at, after: cursor?.id, limit: pageSize })
      cursor = page.at(-1) ?? cursor
      setEvents(shown => [...shown, ...page])
      if (page.length < pageSize) setExhausted(true)
    } catch (e) {
      setError(e)
    } finally {
      setLoading(false)
    }
  }

  onMount(() => void loadMore())

  // Stop showing events, such as those just deleted.
  const drop = (gone: (event: NostrEvent) => boolean) => setEvents(shown => shown.filter(event => !gone(event)))

  return { events, loading, error, exhausted, loadMore, drop }
}
