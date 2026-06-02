import { A } from "@solidjs/router"
import { Show } from "solid-js"

type BackLinkProps = {
  // Omit to pop the previous history entry instead of navigating to a fixed
  // route — for pages reachable from several places, "back" should land the
  // user wherever they came from.
  href?: string
  label: string
}

export default function BackLink(props: BackLinkProps) {
  return (
    <div class="flex items-center gap-2 mb-6">
      <Show
        when={props.href}
        fallback={
          <button type="button" onClick={() => history.back()} class="text-gray-500 hover:text-gray-700">
            ← {props.label}
          </button>
        }
      >
        {(href) => <A href={href()} class="text-gray-500 hover:text-gray-700">← {props.label}</A>}
      </Show>
    </div>
  )
}
