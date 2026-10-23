import { useParams } from "@solidjs/router"
import { createMemo, createSignal, For, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import NoteContent from "@/shared/components/NoteContent"
import PageContainer from "@/shared/components/PageContainer"
import { callRelayMethod, type NostrEvent } from "@/shared/lib/api"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { formatTimeAgo } from "@/shared/lib/format"
import { kindLabel } from "@/shared/lib/kinds"
import { shortenNpub } from "@/shared/lib/pubkey"
import { loaded } from "@/shared/lib/resource"
import { useFramedRelay } from "@/tenant/components/RelayFrame"
import FeedFooter from "@/tenant/components/relay/FeedFooter"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import { makeChange } from "@/tenant/lib/changes"
import usePagedEvents from "@/tenant/lib/usePagedEvents"

// Everything a relay holds, newest first, with what the relay writes about itself left out.
export default function RelayContent() {
  const params = useParams()
  const [relay] = useFramedRelay()
  const feed = usePagedEvents(() => params.id!)
  const [banning, setBanning] = createSignal<string>()
  const [busy, setBusy] = createSignal(false)

  const shown = createMemo(() => feed.events().filter(event => event.pubkey !== loaded(relay)?.pubkey))

  async function remove(event: NostrEvent) {
    if (await makeChange(() => callRelayMethod(params.id!, "banevent", [event.id, ""]), "Failed to delete the event", "Event deleted")) {
      feed.drop(shown => shown.id === event.id)
    }
  }

  async function ban() {
    const pubkey = banning()
    if (!pubkey) return

    setBusy(true)
    const banned = await makeChange(() => callRelayMethod(params.id!, "banpubkey", [pubkey, ""]), "Failed to ban the user", "User banned")
    setBusy(false)
    if (banned) {
      setBanning(undefined)
      feed.drop(event => event.pubkey === pubkey)
    }
  }

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900">Content</h1>
      <p class="mb-6 mt-1 text-sm text-gray-500">Everything published to the relay, newest first.</p>

      <ul class="space-y-3">
        <For each={shown()}>
          {(event) => (
            <li class="rounded-xl border border-gray-200 bg-white p-4">
              <div class="flex items-start justify-between gap-3">
                <PubkeyLabel pubkey={event.pubkey} />
                <div class="flex shrink-0 items-center gap-2">
                  <span class="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-0.5 text-xs font-medium text-gray-700">{kindLabel(event.kind)}</span>
                  <span class="text-xs text-gray-500">{formatTimeAgo(event.created_at)}</span>
                  <ActionsMenu>
                    {(close) => (
                      <>
                        <ActionsMenuItem onClick={() => { close(); void copyToClipboard(JSON.stringify(event, null, 2), { successMessage: "Event copied" }) }}>
                          Copy event JSON
                        </ActionsMenuItem>
                        <ActionsMenuItem tone="danger" onClick={() => { close(); void remove(event) }}>Delete event</ActionsMenuItem>
                        <ActionsMenuItem tone="danger" onClick={() => { close(); setBanning(event.pubkey) }}>Ban author</ActionsMenuItem>
                      </>
                    )}
                  </ActionsMenu>
                </div>
              </div>
              <Show when={event.kind !== 0 && event.content.trim()}>
                <div class="mt-3">
                  <NoteContent content={event.content} tags={event.tags} />
                </div>
              </Show>
            </li>
          )}
        </For>
      </ul>

      <FeedFooter
        count={shown().length}
        loading={feed.loading()}
        error={feed.error()}
        exhausted={feed.exhausted()}
        empty="Nothing has been published yet."
        onMore={() => void feed.loadMore()}
      />

      <ConfirmDialog
        confirmation={banning() ? {
          title: "Ban this user?",
          description: `${shortenNpub(banning()!)} won't be able to use the relay.`,
          details: ["Everything they've published to it is deleted.", "They're removed as a member.", "You can lift the ban from the Bans page."],
          confirmLabel: "Yes, ban",
          busyLabel: "Banning...",
          tone: "danger",
        } : undefined}
        busy={busy()}
        onConfirm={ban}
        onClose={() => !busy() && setBanning(undefined)}
      />
    </PageContainer>
  )
}
