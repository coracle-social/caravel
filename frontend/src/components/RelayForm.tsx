import { createEffect, createSignal } from "solid-js"
import type { Relay } from "@/lib/hooks"
import { slugify } from "@/lib/slugify"
import { PLANS } from "@/lib/api"

export type RelayFormValues = Pick<Relay, "info_name" | "subdomain" | "info_icon" | "info_description" | "plan">

type RelayFormProps = {
  initialValues?: Partial<RelayFormValues>
  syncSubdomainWithName?: boolean
  onSubmit: (values: RelayFormValues) => Promise<void> | void
  submitLabel: string
  submittingLabel: string
}

export default function RelayForm(props: RelayFormProps) {
  const [plan, setPlan] = createSignal(props.initialValues?.plan ?? "")
  const [name, setName] = createSignal(props.initialValues?.info_name ?? "")
  const [subdomain, setSubdomain] = createSignal(props.initialValues?.subdomain ?? "")
  const [icon, setIcon] = createSignal(props.initialValues?.info_icon ?? "")
  const [description, setDescription] = createSignal(props.initialValues?.info_description ?? "")
  const [submitting, setSubmitting] = createSignal(false)
  const [error, setError] = createSignal("")

  async function handleSubmit(e: Event) {
    e.preventDefault()
    setError("")
    setSubmitting(true)

    try {
      await props.onSubmit({
        plan: plan(),
        info_name: name(),
        subdomain: subdomain(),
        info_icon: icon(),
        info_description: description(),
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save relay")
    } finally {
      setSubmitting(false)
    }
  }

  createEffect(() => {
    if (props.syncSubdomainWithName) {
      setSubdomain(slugify(name()))
    }
  })

  return (
    <form onSubmit={handleSubmit} class="space-y-6">
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
        <input
          required
          value={name()}
          onInput={e => setName(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Subdomain</label>
        <div class="flex items-center border border-gray-300 rounded-lg overflow-hidden">
          <input
            value={subdomain()}
            onInput={e => setSubdomain(e.currentTarget.value)}
            required
            class="flex-1 px-3 py-2"
          />
          <span class="px-3 py-2 bg-gray-50 text-gray-500 text-sm border-l border-gray-300">.spaces.coracle.social</span>
        </div>
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Icon URL</label>
        <input
          type="url"
          value={icon()}
          onInput={e => setIcon(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Description</label>
        <textarea
          rows={3}
          value={description()}
          onInput={e => setDescription(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-3">Plan</label>
        <div class="grid grid-cols-3 gap-3">
          {PLANS.map(p => (
            <button
              type="button"
              onClick={() => setPlan(p.id)}
              class={`border-2 rounded-xl p-4 text-left transition-colors ${
                plan() === p.id
                  ? "border-blue-600 bg-blue-50"
                  : "border-gray-200 hover:border-gray-300"
              }`}
            >
              <div class="font-bold text-gray-900">{p.label}</div>
              <div class="text-sm text-gray-500 mt-1">
                {p.price === 0 ? "Free" : `${p.price.toLocaleString()} sats/mo`}
              </div>
              <div class="text-xs text-gray-500 mt-2">{p.members} members</div>
            </button>
          ))}
        </div>
      </div>
      {error() && <p class="text-sm text-red-600">{error()}</p>}
      <button
        type="submit"
        disabled={submitting()}
        class="w-full py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50"
      >
        {submitting() ? props.submittingLabel : props.submitLabel}
      </button>
    </form>
  )
}
