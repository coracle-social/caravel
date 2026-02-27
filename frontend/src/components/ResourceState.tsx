import { Show } from "solid-js"
import LoadingState from "./LoadingState"

type ResourceStateProps = {
  loading: boolean
  error: unknown
  loadingText: string
  errorText: string
  class?: string
}

export default function ResourceState(props: ResourceStateProps) {
  return (
    <>
      <Show when={props.loading}>
        <LoadingState message={props.loadingText} class={props.class} />
      </Show>
      <Show when={props.error && !props.loading}>
        <p class={`text-center text-red-600 ${props.class ?? ""}`.trim()}>{props.errorText}</p>
      </Show>
    </>
  )
}
