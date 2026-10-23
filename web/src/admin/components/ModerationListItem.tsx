import { A } from "@solidjs/router"
import { createSignal, onCleanup, onMount, Show } from "solid-js"
import type { ProfileContent } from "applesauce-core/helpers/profile"
import { canonicalHost, fetchModerationBlob, relayName, type ModerationItem, type Relay, type Upload } from "@/shared/lib/api"
import Avatar from "@/shared/components/Avatar"
import CopyNpub from "@/shared/components/CopyNpub"
import { profileName } from "@/shared/lib/profiles"
import NoteContent from "@/shared/components/NoteContent"
import { formatBytes, formatTimeAgo } from "@/shared/lib/format"
import { kindLabel } from "@/shared/lib/kinds"

function FileIcon() {
  return (
    <svg class="h-7 w-7 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  )
}

// An upload as an image, unless its type rules that out or it won't load, or else as a file tile.
function UploadPreview(props: { upload: Upload; relayId: string }) {
  const [src, setSrc] = createSignal<string>()
  const [failed, setFailed] = createSignal(false)
  const couldBeImage = !props.upload.type || props.upload.type.startsWith("image/")

  let anchor: HTMLAnchorElement | undefined
  let objectUrl: string | undefined
  let disposed = false

  onCleanup(() => {
    disposed = true
    if (objectUrl) URL.revokeObjectURL(objectUrl)
  })

  onMount(() => {
    if (!couldBeImage || !anchor) return

    // Each preview proxies a whole blob, so fetch only what scrolls into view.
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return

        observer.disconnect()

        fetchModerationBlob(props.relayId, props.upload.sha256)
          .then((url) => {
            // Revoke rather than leak if the row went away mid-flight.
            if (disposed) return URL.revokeObjectURL(url)

            objectUrl = url
            setSrc(url)
          })
          .catch(() => setFailed(true))
      },
      {rootMargin: "300px"},
    )

    observer.observe(anchor)

    onCleanup(() => observer.disconnect())
  })

  return (
    <a
      ref={anchor}
      // The proxied bytes once loaded, or else the blob on its relay, which only members can open.
      href={src() ?? props.upload.url}
      target="_blank"
      rel="noreferrer"
      title="Open in a new tab"
      class="flex h-20 w-20 shrink-0 items-center justify-center overflow-hidden rounded border border-gray-200 bg-gray-50 hover:border-gray-300"
    >
      <Show when={src() && !failed()} fallback={<FileIcon />}>
        <img src={src()} alt="" class="h-full w-full object-cover" onError={() => setFailed(true)} />
      </Show>
    </a>
  )
}

type ModerationListItemProps = {
  item: ModerationItem
  relay?: Relay
  metadata?: ProfileContent
}

export default function ModerationListItem(props: ModerationListItemProps) {
  const displayName = () => profileName(props.metadata)
  const label = () =>
    props.item.type === "upload"
      ? "Upload"
      : kindLabel(props.item.data.kind)

  return (
    <li class="rounded-lg border border-gray-200 bg-white p-4">
      <div class="flex items-start justify-between gap-3">
        <div class="flex min-w-0 items-center gap-2">
          <Avatar profile={props.metadata} pubkey={props.item.data.pubkey} class="h-7 w-7 flex-shrink-0" fallbackClass="bg-gray-200 text-[11px] text-gray-500" />
          <div class="min-w-0">
            <Show when={displayName()}>
              <p class="truncate text-sm font-medium text-gray-900">{displayName()}</p>
            </Show>
            <CopyNpub pubkey={props.item.data.pubkey} class="text-xs text-gray-500" />
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <span class="inline-flex items-center rounded-full border border-gray-200 bg-gray-50 px-2.5 py-0.5 text-xs font-medium text-gray-700">
            {label()}
          </span>
          <span class="text-xs text-gray-500">{formatTimeAgo(props.item.seen_at)}</span>
        </div>
      </div>

      <Show when={props.item.type === "event" ? props.item.data : undefined}>
        {(event) => (
          <Show
            when={event().kind !== 0}
            fallback={
              <p class="mt-3 text-sm italic text-gray-400">
                {displayName() || "This user"} updated their profile
              </p>
            }
          >
            <Show
              when={event().content.trim()}
              fallback={<p class="mt-3 text-sm italic text-gray-400">No content</p>}
            >
              <div class="mt-3">
                <NoteContent content={event().content} tags={event().tags} />
              </div>
            </Show>
          </Show>
        )}
      </Show>

      <Show when={props.item.type === "upload" ? props.item.data : undefined}>
        {(upload) => (
          <div class="mt-3 flex items-start gap-3">
            <UploadPreview upload={upload()} relayId={props.item.relay_id} />
            <div class="min-w-0 text-sm">
              <a href={upload().url} target="_blank" rel="noreferrer" class="block truncate text-blue-600 hover:underline">
                {upload().sha256}
              </a>
              <p class="text-xs text-gray-500">
                {upload().type || "unknown type"} · {formatBytes(upload().size)}
              </p>
            </div>
          </div>
        )}
      </Show>

      <div class="mt-3 border-t border-gray-100 pt-3">
        <A href={`/admin/relays/${props.item.relay_id}`} class="text-xs text-gray-500 hover:text-gray-900 hover:underline">
          <Show when={props.relay} fallback={props.item.relay_id}>
            {(relay) => <>{relayName(relay())} · {canonicalHost(relay())}</>}
          </Show>
        </A>
      </div>
    </li>
  )
}
