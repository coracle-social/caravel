import { Show } from "solid-js"
import LoadingState from "@/shared/components/LoadingState"

// The end of a paged list: loading, a failure, a button for more, or that there is no more.
export default function FeedFooter(props: {
  count: number
  loading: boolean
  error: unknown
  exhausted: boolean
  empty: string
  onMore: () => void
}) {
  return (
    <Show
      when={props.count > 0}
      fallback={
        <Show
          when={props.loading}
          fallback={<p class={`py-20 text-center ${props.error ? "text-red-600" : "text-gray-500"}`}>{props.error ? "Failed to load." : props.empty}</p>}
        >
          <LoadingState message="Loading..." />
        </Show>
      }
    >
      <div class="mt-6 text-center">
        <Show when={props.error}>
          <p class="mb-3 text-sm text-red-600">Failed to load more.</p>
        </Show>
        <Show when={!props.exhausted} fallback={<p class="text-sm text-gray-500">That's everything.</p>}>
          <button
            type="button"
            onClick={props.onMore}
            disabled={props.loading}
            class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50 disabled:opacity-50"
          >
            {props.loading ? "Loading..." : "Load more"}
          </button>
        </Show>
      </div>
    </Show>
  )
}
