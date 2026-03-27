import { createEffect, createSignal } from "solid-js"
import type { Relay } from "@/lib/hooks"
import { slugify } from "@/lib/slugify"

export type RelayFormValues = {
  info_name: string
  subdomain: string
  info_icon: string
  info_description: string
}

type RelayFormProps = {
  initialValues: Pick<Relay, "info_name" | "subdomain" | "info_icon" | "info_description">
  syncSubdomainWithName?: boolean
  onSubmit: (values: RelayFormValues, e: Event) => void | Promise<void>
  submitting: boolean
  error?: string
  submitLabel: string
  submittingLabel: string
}

export default function RelayForm(props: RelayFormProps) {
  const [name, setName] = createSignal(props.initialValues.info_name)
  const [subdomain, setSubdomain] = createSignal(props.initialValues.subdomain)
  const [icon, setIcon] = createSignal(props.initialValues.info_icon)
  const [description, setDescription] = createSignal(props.initialValues.info_description)

  createEffect(() => {
    setName(props.initialValues.info_name)
    setSubdomain(props.initialValues.subdomain)
    setIcon(props.initialValues.info_icon)
    setDescription(props.initialValues.info_description)
  })

  function handleSubmit(e: Event) {
    e.preventDefault()
    void props.onSubmit(
      {
        info_name: name(),
        subdomain: subdomain(),
        info_icon: icon(),
        info_description: description(),
      },
      e,
    )
  }

  return (
    <form onSubmit={handleSubmit} class="space-y-6">
      {/* Basic info */}
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
        <input
          value={name()}
          onInput={e => {
            const value = e.currentTarget.value
            setName(value)
            if (props.syncSubdomainWithName) setSubdomain(slugify(value))
          }}
          required
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
          value={description()}
          onInput={e => setDescription(e.currentTarget.value)}
          rows={3}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      {props.error && <p class="text-sm text-red-600">{props.error}</p>}
      <button
        type="submit"
        disabled={props.submitting}
        class="w-full py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50"
      >
        {props.submitting ? props.submittingLabel : props.submitLabel}
      </button>
    </form>
  )
}
