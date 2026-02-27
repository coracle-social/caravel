import { Show } from "solid-js"

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
        <p class={`text-gray-500 ${props.class ?? ""}`.trim()}>{props.loadingText}</p>
      </Show>
      <Show when={props.error && !props.loading}>
        <p class={`text-red-600 ${props.class ?? ""}`.trim()}>{props.errorText}</p>
      </Show>
    </>
  )
}
