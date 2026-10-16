type LoadingStateProps = {
  message: string
  class?: string
  paddingClass?: string
}

export default function LoadingState(props: LoadingStateProps) {
  const paddingClass = () => props.paddingClass ?? "py-20"
  return (
    <div class={`flex items-center justify-center gap-2 ${paddingClass()} text-gray-500 ${props.class ?? ""}`.trim()}>
      <div class="h-4 w-4 animate-spin rounded-full border-2 border-gray-300 border-t-gray-700" />
      <p class="text-sm text-gray-500">{props.message}</p>
    </div>
  )
}
