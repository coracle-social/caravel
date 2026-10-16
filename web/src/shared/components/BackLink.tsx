import { A } from "@solidjs/router"
import { Show } from "solid-js"

type BackLinkProps = {
  // Without one, going back returns to wherever the page was opened from.
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
