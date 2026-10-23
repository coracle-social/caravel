import { createSignal, onMount } from "solid-js"
import { listRelayEvents, type NostrEvent } from "@/shared/lib/api"

// A relay's events of some kinds (any, if none), newest first, a page at a time.
export default function usePagedEvents(relayId: () => string, kinds: number[] = [], pageSize = 30) {
  const [events, setEvents] = createSignal<NostrEvent[]>([])
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<unknown>()
  const [exhausted, setExhausted] = createSignal(false)

  async function loadMore() {
    if (loading() || exhausted()) return

    setLoading(true)
    setError(undefined)
    try {
      const shown = events()
      const page = await listRelayEvents(relayId(), { kinds, until: shown[shown.length - 1]?.created_at, limit: pageSize })

      // `until` is inclusive, so a page repeats the events that share its last second.
      const seen = new Set(shown.map(event => event.id))
      const fresh = page.filter(event => !seen.has(event.id))
      setEvents([...shown, ...fresh])

      if (page.length < pageSize || fresh.length === 0) setExhausted(true)
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
