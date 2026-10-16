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

// Long entity strings (invoices, nevents) are unreadable in full and only need
// to be recognizable at a glance.
const shorten = (value: string) => (value.length > 24 ? `${value.slice(0, 24)}…` : value)

function Link(props: { value: ParsedLinkValue; raw: string }) {
  const url = props.value.url

  // Only hand the browser a scheme it should follow. Anything else — javascript:,
  // data:, whatever else a poster invented — stays inert text.
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return <>{props.raw}</>
  }

  return (
    <a
      href={url.toString()}
      target="_blank"
      rel="noreferrer nofollow"
      // Show what was written, but reveal where it actually goes on hover: the
      // two differing is itself worth seeing when you're moderating.
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

  // Custom emoji resolve to an image on a poster-controlled host; render the
  // shortcode instead of fetching from one.
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

  // Whatever's left — text, newlines, emails — is a plain string, and the
  // container preserves the whitespace.
  return <>{parsed.value}</>
}

type NoteContentProps = {
  content: string
  tags: string[][]
}

// Renders note content as text with clickable links, hashtags and mentions.
// Built up from parsed segments rather than welshman's HTML renderer, so
// nothing a stranger authored is ever handed to innerHTML.
export default function NoteContent(props: NoteContentProps) {
  // Truncated as well as line-clamped: the clamp only hides overflow, while a
  // feed page can carry a thousand of these and each one's DOM is real.
  const parsed = createMemo(() =>
    truncate(parse({content: props.content, tags: props.tags}), {minLength: 300, maxLength: 500}),
  )

  return (
    <p class="whitespace-pre-wrap break-words text-sm text-gray-800 line-clamp-6">
      <For each={parsed()}>{(segment) => <Segment parsed={segment} />}</For>
    </p>
  )
}
