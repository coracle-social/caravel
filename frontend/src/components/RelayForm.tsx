type RelayFormProps = {
  name: string
  setName: (value: string) => void
  subdomain: string
  setSubdomain: (value: string) => void
  icon: string
  setIcon: (value: string) => void
  description: string
  setDescription: (value: string) => void
  plan: string
  setPlan: (value: string) => void
  plans: readonly string[]
  onSubmit: (e: Event) => void
  submitting: boolean
  error?: string
  submitLabel: string
  submittingLabel: string
}

export default function RelayForm(props: RelayFormProps) {
  return (
    <form onSubmit={props.onSubmit} class="space-y-4">
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
        <input
          value={props.name}
          onInput={e => props.setName(e.currentTarget.value)}
          required
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Subdomain</label>
        <div class="flex items-center border border-gray-300 rounded-lg overflow-hidden">
          <input
            value={props.subdomain}
            onInput={e => props.setSubdomain(e.currentTarget.value)}
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
          value={props.icon}
          onInput={e => props.setIcon(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Description</label>
        <textarea
          value={props.description}
          onInput={e => props.setDescription(e.currentTarget.value)}
          rows={3}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-2">Plan</label>
        <div class="flex gap-2">
          {props.plans.map(p => (
            <button
              type="button"
              class={`rounded-lg border px-3 py-2 text-sm capitalize ${props.plan === p ? "border-blue-600 text-blue-700" : "border-gray-300 text-gray-700"}`}
              onClick={() => props.setPlan(p)}
            >
              {p}
            </button>
          ))}
        </div>
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
