import { Show, type JSX } from "solid-js"

// A titled card on a relay's pages, with an action beside its title.
export default function Section(props: { title: string; description?: string; action?: JSX.Element; children: JSX.Element }) {
  return (
    <section class="rounded-xl border border-gray-200 bg-white p-6">
      <div class="mb-4 flex items-start justify-between gap-4">
        <div>
          <h2 class="text-lg font-semibold text-gray-900">{props.title}</h2>
          <Show when={props.description}>
            <p class="mt-1 text-sm text-gray-500">{props.description}</p>
          </Show>
        </div>
        {props.action}
      </div>
      {props.children}
    </section>
  )
}
