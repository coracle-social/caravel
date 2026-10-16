import { Show } from "solid-js"
import type { Interval } from "@/portal/lib/api"
import { formatPrice } from "@/portal/lib/format"

type Described = {
  host: string
  name: string
  icon: string
  description: string
  price?: number
  interval?: Interval
}

// Which relay this is, and what membership costs when it sells one.
export default function RelayHeader(props: { relay: Described }) {
  const name = () => props.relay.name || props.relay.host

  return (
    <div class="space-y-4">
      <div class="flex items-center gap-4">
        <Show
          when={props.relay.icon}
          fallback={
            <div class="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-zinc-100 text-xl font-semibold text-zinc-500">
              {name().slice(0, 1).toUpperCase()}
            </div>
          }
        >
          <img src={props.relay.icon} alt="" class="h-14 w-14 shrink-0 rounded-xl object-cover" />
        </Show>
        <div class="min-w-0">
          <h1 class="break-words text-xl font-semibold">{name()}</h1>
          <p class="break-all text-sm text-zinc-500">{props.relay.host}</p>
        </div>
      </div>
      <Show when={props.relay.description}>
        <p class="whitespace-pre-line text-sm leading-6 text-zinc-600">{props.relay.description}</p>
      </Show>
      <Show when={props.relay.price && props.relay.interval}>
        <p class="text-2xl font-semibold">{formatPrice(props.relay.price!, props.relay.interval!)}</p>
      </Show>
    </div>
  )
}
