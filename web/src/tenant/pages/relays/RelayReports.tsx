import { useParams } from "@solidjs/router"
import { createMemo, createResource, For, Show } from "solid-js"
import { uniq } from "@welshman/lib"
import NoteContent from "@/shared/components/NoteContent"
import PageContainer from "@/shared/components/PageContainer"
import { callRelayMethod, listRelayEvents, type NostrEvent } from "@/shared/lib/api"
import { formatTimeAgo } from "@/shared/lib/format"
import { loaded } from "@/shared/lib/resource"
import FeedFooter from "@/tenant/components/relay/FeedFooter"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import { makeChange } from "@/tenant/lib/changes"
import usePagedEvents from "@/tenant/lib/usePagedEvents"

const REPORT = 1984

const tagValue = (event: NostrEvent, name: string, index = 1) => event.tags.find(tag => tag[0] === name)?.[index]

// What a report says is wrong: the type its first tag gives, or else nothing.
const reportReason = (report: NostrEvent) => tagValue(report, "e", 2) || tagValue(report, "p", 2) || ""

// Reports (NIP-56) members sent the relay, and what to do about each one.
export default function RelayReports() {
  const params = useParams()
  const feed = usePagedEvents(() => params.id!, [REPORT], 20)

  // The reported events the relay still holds, fetched for each page of reports.
  const targetIds = createMemo(() => uniq(feed.events().flatMap(report => report.tags.filter(tag => tag[0] === "e").map(tag => tag[1]))))
  const [targets] = createResource(
    () => (targetIds().length ? targetIds() : undefined),
    (ids) => listRelayEvents(params.id!, { ids, limit: 100 }),
  )
  const target = (id?: string) => (loaded(targets) ?? []).find(event => event.id === id)

  async function dismiss(report: NostrEvent) {
    await callRelayMethod(params.id!, "banevent", [report.id, "Dismissed"])
    feed.drop(event => event.id === report.id)
  }

  async function deleteTarget(report: NostrEvent, id: string) {
    const deleted = await makeChange(async () => {
      await callRelayMethod(params.id!, "banevent", [id, reportReason(report)])
      await dismiss(report)
    }, "Failed to delete the event", "Event deleted")
    if (deleted) feed.drop(event => event.tags.some(tag => tag[0] === "e" && tag[1] === id))
  }

  async function banAuthor(report: NostrEvent, pubkey: string) {
    const banned = await makeChange(async () => {
      await callRelayMethod(params.id!, "banpubkey", [pubkey, reportReason(report)])
      await dismiss(report)
    }, "Failed to ban the user", "User banned")
    if (banned) feed.drop(event => event.pubkey === pubkey || event.tags.some(tag => tag[0] === "p" && tag[1] === pubkey))
  }

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900">Reports</h1>
      <p class="mb-6 mt-1 text-sm text-gray-500">What members reported to the relay. Dismissing a report deletes it.</p>

      <ul class="space-y-3">
        <For each={feed.events()}>
          {(report) => {
            const targetId = tagValue(report, "e")
            const reported = () => target(targetId)
            const author = () => reported()?.pubkey ?? tagValue(report, "p")

            return (
              <li class="space-y-3 rounded-xl border border-gray-200 bg-white p-4">
                <div class="flex flex-wrap items-center justify-between gap-2 text-sm text-gray-500">
                  <div class="flex items-center gap-2">
                    <span>Reported by</span>
                    <PubkeyLabel pubkey={report.pubkey} />
                  </div>
                  <div class="flex items-center gap-2">
                    <Show when={reportReason(report)}>
                      <span class="rounded-full bg-red-50 px-2 py-0.5 text-xs font-medium text-red-700">{reportReason(report)}</span>
                    </Show>
                    <span class="text-xs">{formatTimeAgo(report.created_at)}</span>
                  </div>
                </div>

                <div class="rounded-lg bg-gray-50 p-3">
                  <Show
                    when={reported()}
                    fallback={
                      <Show
                        when={targetId}
                        fallback={<Show when={author()}>{(pubkey) => <PubkeyLabel pubkey={pubkey()} />}</Show>}
                      >
                        <p class="text-sm italic text-gray-500">{targets.loading ? "Loading the reported event..." : "The reported event isn't on this relay."}</p>
                      </Show>
                    }
                  >
                    {(event) => (
                      <div class="space-y-2">
                        <PubkeyLabel pubkey={event().pubkey} />
                        <NoteContent content={event().content} tags={event().tags} />
                      </div>
                    )}
                  </Show>
                </div>

                <Show when={report.content.trim()}>
                  <div class="text-sm text-gray-700">
                    <p class="text-xs font-medium uppercase tracking-wide text-gray-500">The reporter adds</p>
                    <NoteContent content={report.content} tags={report.tags} />
                  </div>
                </Show>

                <div class="flex flex-wrap justify-end gap-2">
                  <button type="button" onClick={() => void makeChange(() => dismiss(report), "Failed to dismiss the report", "Report dismissed")} class="rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
                    Dismiss
                  </button>
                  <Show when={reported()}>
                    {(event) => (
                      <button type="button" onClick={() => void deleteTarget(report, event().id)} class="rounded-lg border border-red-200 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50">
                        Delete event
                      </button>
                    )}
                  </Show>
                  <Show when={author()}>
                    {(pubkey) => (
                      <button type="button" onClick={() => void banAuthor(report, pubkey())} class="rounded-lg bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700">
                        Ban {reported() ? "author" : "user"}
                      </button>
                    )}
                  </Show>
                </div>
              </li>
            )
          }}
        </For>
      </ul>

      <FeedFooter
        count={feed.events().length}
        loading={feed.loading()}
        error={feed.error()}
        exhausted={feed.exhausted()}
        empty="No reports."
        onMore={() => void feed.loadMore()}
      />
    </PageContainer>
  )
}
