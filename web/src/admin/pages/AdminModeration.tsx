import { createMemo, createSignal, For, onMount, Show } from "solid-js"
import { uniq } from "@welshman/lib"
import { listModeration, type ModerationItem } from "@/shared/lib/api"
import LoadingState from "@/shared/components/LoadingState"
import ModerationListItem from "@/admin/components/ModerationListItem"
import PageContainer from "@/shared/components/PageContainer"
import SearchInput from "@/shared/components/SearchInput"
import { fuzzySearch } from "@/shared/lib/search"
import { useAdminRelays, useProfileMetadataMap } from "@/shared/lib/hooks"

const TYPE_FILTERS = [
  {value: "all", label: "All"},
  {value: "event", label: "Events"},
  {value: "upload", label: "Uploads"},
] as const

// How far back the feed reaches. A bounded window keeps the relay from having
// to scan its whole history to answer, and gives "load more" a real end.
const RANGES = [
  {value: "1d", label: "24 hours", secs: 24 * 60 * 60},
  {value: "7d", label: "7 days", secs: 7 * 24 * 60 * 60},
  {value: "30d", label: "30 days", secs: 30 * 24 * 60 * 60},
  {value: "all", label: "All time", secs: undefined},
] as const

type TypeFilter = (typeof TYPE_FILTERS)[number]["value"]
type RangeValue = (typeof RANGES)[number]["value"]

// Frozen when the range is picked rather than recomputed per request, so
// paging backwards can't drag the floor along with it.
const startOfRange = (range: RangeValue) => {
  const secs = RANGES.find(candidate => candidate.value === range)?.secs
  return secs === undefined ? undefined : Math.floor(Date.now() / 1000) - secs
}

// Uploads have no event id, so key them on the blob hash instead. Scoped by
// relay because the same content can legitimately exist on several of them.
const itemKey = (item: ModerationItem) =>
  item.type === "upload" ? `${item.relay_id}:${item.data.sha256}` : `${item.relay_id}:${item.data.id}`

// Relay machinery rather than user content: role/permission events (8000/8001),
// relay lists (10002), group lists (10009), and 13534. NIP-29's group management
// events (9000-9020 moderation, 9021/9022 join/leave) are matched as a range.
const CONTROL_KINDS = new Set([8000, 8001, 10002, 10009, 13534])

const isControlEvent = (item: ModerationItem) =>
  item.type === "event" &&
  (CONTROL_KINDS.has(item.data.kind) || (item.data.kind >= 9000 && item.data.kind <= 9022))

export default function AdminModeration() {
  const [items, setItems] = createSignal<ModerationItem[]>([])
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<unknown>()
  const [exhausted, setExhausted] = createSignal(false)
  const [query, setQuery] = createSignal("")
  const [typeFilter, setTypeFilter] = createSignal<TypeFilter>("all")
  const [range, setRange] = createSignal<RangeValue>("1d")
  const [since, setSince] = createSignal<number | undefined>(startOfRange("1d"))
  const [relays] = useAdminRelays()

  // Bumped whenever the window changes, so a response for the old window can't
  // land on top of the new one.
  let generation = 0

  const relaysById = createMemo(() => new Map((relays() ?? []).map(relay => [relay.id, relay])))

  // Control events stay in `items` so the pagination cursor and dedupe still see
  // them; they're only hidden from the feed.
  const visible = createMemo(() => items().filter(item => !isControlEvent(item)))
  const metadata = useProfileMetadataMap(() => uniq(visible().map(item => item.data.pubkey)))

  const loadMore = async () => {
    if (loading() || exhausted()) return

    const current = generation

    setLoading(true)
    setError(undefined)

    try {
      const loaded = items()
      const oldest = loaded[loaded.length - 1]
      const page = await listModeration({since: since(), until: oldest?.seen_at})

      if (current !== generation) return

      // `until` is inclusive and seen_at only has second resolution, so a page
      // boundary repeats every item sharing that second.
      const seen = new Set(loaded.map(itemKey))
      const fresh = page.items.filter(item => !seen.has(itemKey(item)))

      setItems(prev => [...prev, ...fresh])

      // Either the window is fully covered, or a whole page shares one second
      // and `until` has nowhere further back to go.
      if (!page.truncated || fresh.length === 0) setExhausted(true)
    } catch (e) {
      if (current === generation) setError(e)
    } finally {
      if (current === generation) setLoading(false)
    }
  }

  const selectRange = (value: RangeValue) => {
    if (value === range()) return

    generation += 1

    setRange(value)
    setSince(startOfRange(value))
    setItems([])
    setExhausted(false)
    setError(undefined)
    setLoading(false)

    void loadMore()
  }

  onMount(() => void loadMore())

  // Filtering is over what's been loaded, not the whole window, so paging
  // further back can surface more matches.
  const filtered = createMemo(() => {
    const type = typeFilter()
    const matching = type === "all" ? visible() : visible().filter(item => item.type === type)
    const q = query().trim()

    if (!q) return matching

    const searchable = matching.map(item => ({
      item,
      pubkey: item.data.pubkey,
      author: metadata()[item.data.pubkey]?.name ?? metadata()[item.data.pubkey]?.display_name ?? "",
      relay: relaysById().get(item.relay_id)?.info_name ?? item.relay_id,
      content: item.type === "event" ? item.data.content : `${item.data.type} ${item.data.sha256}`,
    }))

    return fuzzySearch(searchable, ["pubkey", "author", "relay", "content"], q).map(result => result.item)
  })

  const typeButtonClass = (value: TypeFilter) =>
    typeFilter() === value
      ? "rounded-lg bg-gray-900 px-3 py-1.5 text-sm font-medium text-white"
      : "rounded-lg px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-100"

  const rangeButtonClass = (value: RangeValue) =>
    range() === value
      ? "rounded-lg border border-gray-900 px-3 py-1.5 text-sm font-medium text-gray-900"
      : "rounded-lg border border-transparent px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-100"

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Moderation</h1>

      <div class="mb-6 space-y-3">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search content, author, or relay..." />
        <div class="flex flex-wrap items-center justify-between gap-y-2">
          <div class="flex items-center gap-1">
            <For each={TYPE_FILTERS}>
              {(filter) => (
                <button type="button" class={typeButtonClass(filter.value)} onClick={() => setTypeFilter(filter.value)}>
                  {filter.label}
                </button>
              )}
            </For>
          </div>
          <div class="flex items-center gap-1">
            <For each={RANGES}>
              {(option) => (
                <button type="button" class={rangeButtonClass(option.value)} onClick={() => selectRange(option.value)}>
                  {option.label}
                </button>
              )}
            </For>
          </div>
        </div>
      </div>

      <Show when={error() && items().length === 0}>
        <p class="text-center text-red-600">Failed to load moderation feed.</p>
      </Show>

      <Show when={loading() && items().length === 0}>
        <LoadingState message="Loading recent activity..." />
      </Show>

      <Show when={items().length > 0 || (!loading() && !error())}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">Nothing to moderate.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(item) => (
                <ModerationListItem
                  item={item}
                  relay={relaysById().get(item.relay_id)}
                  metadata={metadata()[item.data.pubkey]}
                />
              )}
            </For>
          </ul>
        </Show>
      </Show>

      <Show when={items().length > 0}>
        <div class="mt-6 text-center">
          <Show when={error()}>
            <p class="mb-3 text-sm text-red-600">Failed to load more.</p>
          </Show>
          <Show
            when={!exhausted()}
            fallback={<p class="text-sm text-gray-500">That's everything in this range.</p>}
          >
            <button
              type="button"
              class="rounded-lg border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
              onClick={loadMore}
              disabled={loading()}
            >
              {loading() ? "Loading..." : "Load more"}
            </button>
          </Show>
        </div>
      </Show>
    </PageContainer>
  )
}
