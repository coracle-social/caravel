import { createMemo, createResource, createSignal, For, onMount, Show } from "solid-js"
import { useSearchParams } from "@solidjs/router"
import { uniq } from "@welshman/lib"
import { listModeration, listRelays, relayHost, type ModerationItem } from "@/shared/lib/api"
import LoadingState from "@/shared/components/LoadingState"
import ModerationListItem from "@/admin/components/ModerationListItem"
import PageContainer from "@/shared/components/PageContainer"
import SearchInput from "@/shared/components/SearchInput"
import { fuzzySearch } from "@/shared/lib/search"
import { useProfileMetadataMap } from "@/shared/lib/profiles"
import { loaded } from "@/shared/lib/resource"

const TYPE_FILTERS = [
  {value: "all", label: "All"},
  {value: "event", label: "Events"},
  {value: "upload", label: "Uploads"},
] as const

// How far back the feed reaches, which bounds the scan and gives "load more" an end.
const RANGES = [
  {value: "1d", label: "24 hours", secs: 24 * 60 * 60},
  {value: "7d", label: "7 days", secs: 7 * 24 * 60 * 60},
  {value: "30d", label: "30 days", secs: 30 * 24 * 60 * 60},
  {value: "all", label: "All time", secs: undefined},
] as const

type TypeFilter = (typeof TYPE_FILTERS)[number]["value"]
type RangeValue = (typeof RANGES)[number]["value"]

// Fixed when the range is picked, so paging back can't move it.
const startOfRange = (range: RangeValue) => {
  const secs = RANGES.find(candidate => candidate.value === range)?.secs
  return secs === undefined ? undefined : Math.floor(Date.now() / 1000) - secs
}

// Uploads are keyed by blob hash, and every item by relay, since relays share content.
const itemKey = (item: ModerationItem) =>
  item.type === "upload" ? `${item.relay_id}:${item.data.sha256}` : `${item.relay_id}:${item.data.id}`

export default function AdminModeration() {
  const [items, setItems] = createSignal<ModerationItem[]>([])
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<unknown>()
  const [exhausted, setExhausted] = createSignal(false)
  const [query, setQuery] = createSignal("")
  const [typeFilter, setTypeFilter] = createSignal<TypeFilter>("all")
  const [range, setRange] = createSignal<RangeValue>("1d")
  const [since, setSince] = createSignal<number | undefined>(startOfRange("1d"))
  const [relays] = createResource(listRelays)
  const [searchParams, setSearchParams] = useSearchParams<{ relay?: string }>()

  // Bumped when the window changes, so a response for the old one is dropped.
  let generation = 0

  const relaysById = createMemo(() => new Map((loaded(relays) ?? []).map(relay => [relay.id, relay])))
  const relayFilter = () => searchParams.relay
  const relayFilterLabel = () => {
    const id = relayFilter() ?? ""
    const relay = relaysById().get(id)
    return relay ? relayHost(relay) : id
  }
  const metadata = useProfileMetadataMap(() => uniq(items().map(item => item.data.pubkey)))

  const loadMore = async () => {
    if (loading() || exhausted()) return

    const current = generation

    setLoading(true)
    setError(undefined)

    try {
      const existing = items()
      const oldest = existing[existing.length - 1]
      const page = await listModeration({since: since(), until: oldest?.seen_at})

      if (current !== generation) return

      // `until` is inclusive, so a page repeats the items that share its last second.
      const seen = new Set(existing.map(itemKey))
      const fresh = page.items.filter(item => !seen.has(itemKey(item)))

      setItems(prev => [...prev, ...fresh])

      // Done when the window is covered, or a whole page shares one second.
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

  // Filters what has loaded, so paging back can find more matches.
  const filtered = createMemo(() => {
    const type = typeFilter()
    const relayId = relayFilter()
    const matching = items().filter(
      item => (type === "all" || item.type === type) && (!relayId || item.relay_id === relayId),
    )
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
            <Show when={relayFilter()}>
              <span class="ml-2 inline-flex items-center gap-2 rounded-lg bg-gray-100 px-3 py-1.5 text-sm text-gray-700">
                {relayFilterLabel()}
                <button
                  type="button"
                  class="text-gray-500 hover:text-gray-900"
                  aria-label="Show every relay"
                  onClick={() => setSearchParams({ relay: undefined })}
                >
                  ×
                </button>
              </span>
            </Show>
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
            fallback={
              <Show when={filtered().length > 0}>
                <p class="text-sm text-gray-500">That's everything in this range.</p>
              </Show>
            }
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
