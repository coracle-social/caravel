import { createMemo, For } from "solid-js"
import {
  isAddress,
  isCashu,
  isCode,
  isEllipsis,
  isEmoji,
  isEvent,
  isInvoice,
  isLink,
  isLinkGrid,
  isProfile,
  isTopic,
  parse,
  truncate,
  type Parsed,
  type ParsedLinkValue,
} from "@welshman/content"
import { shortenNpub } from "@/shared/lib/pubkey"

// Long entities, such as invoices and nevents, only need to be recognizable.
const shorten = (value: string) => (value.length > 24 ? `${value.slice(0, 24)}…` : value)

function Link(props: { value: ParsedLinkValue; raw: string }) {
  const url = props.value.url

  // Any scheme but http and https, such as javascript:, stays inert text.
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return <>{props.raw}</>
  }

  return (
    <a
      href={url.toString()}
      target="_blank"
      rel="noreferrer nofollow"
      // The link text as written, with where it really goes on hover.
      title={url.toString()}
      class="text-blue-600 hover:underline"
    >
      {props.raw}
    </a>
  )
}

function Segment(props: { parsed: Parsed }) {
  const parsed = props.parsed

  if (isLink(parsed)) return <Link value={parsed.value} raw={parsed.raw} />

  if (isLinkGrid(parsed)) {
    return <For each={parsed.value.links}>{(link) => <Link value={link} raw={link.url.toString()} />}</For>
  }

  if (isTopic(parsed)) {
    return <span class="text-blue-600">{parsed.raw}</span>
  }

  if (isProfile(parsed)) {
    return <span class="text-blue-600">@{shortenNpub(parsed.value.pubkey)}</span>
  }

  if (isEvent(parsed) || isAddress(parsed)) {
    return <span class="text-gray-500">{shorten(parsed.raw)}</span>
  }

  // Custom emoji are images on the poster's host, so the shortcode shows instead.
  if (isEmoji(parsed)) {
    return <span class="text-gray-500">:{parsed.value.name}:</span>
  }

  if (isCode(parsed)) {
    return <code class="rounded bg-gray-100 px-1 text-xs">{parsed.value}</code>
  }

  if (isInvoice(parsed) || isCashu(parsed)) {
    return <span class="font-mono text-xs text-gray-500">{shorten(parsed.value)}</span>
  }

  if (isEllipsis(parsed)) return <>…</>

  // Anything else is plain text, whose whitespace the container keeps.
  return <>{parsed.value}</>
}

type NoteContentProps = {
  content: string
  tags: string[][]
}

// A note's content with clickable links, hashtags and mentions, built from
// parsed segments so nothing a stranger wrote reaches innerHTML.
export default function NoteContent(props: NoteContentProps) {
  // Truncated as well as clamped, since a clamp only hides what is still rendered.
  const parsed = createMemo(() =>
    truncate(parse({content: props.content, tags: props.tags}), {minLength: 300, maxLength: 500}),
  )

  return (
    <p class="whitespace-pre-wrap break-words text-sm text-gray-800 line-clamp-6">
      <For each={parsed()}>{(segment) => <Segment parsed={segment} />}</For>
    </p>
  )
}
