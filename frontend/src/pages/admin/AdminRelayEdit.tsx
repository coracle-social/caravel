import { A, useNavigate, useParams } from "@solidjs/router"
import { Show, createEffect, createResource, createSignal } from "solid-js"
import { adminGetRelay, adminUpdateRelay } from "../../lib/api"

const PLANS = ["free", "basic", "growth"] as const
type PlanId = (typeof PLANS)[number]

function slugify(s: string) {
  return s
    .toLowerCase()
    .trim()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
}

export default function AdminRelayEdit() {
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = createResource(relayId, adminGetRelay)

  const [name, setName] = createSignal("")
  const [subdomain, setSubdomain] = createSignal("")
  const [icon, setIcon] = createSignal("")
  const [description, setDescription] = createSignal("")
  const [plan, setPlan] = createSignal<PlanId>("free")
  const [error, setError] = createSignal("")
  const [submitting, setSubmitting] = createSignal(false)

  createEffect(() => {
    const data = relay()
    if (!data) return
    setName(data.name)
    setSubdomain(data.subdomain)
    setIcon(data.icon)
    setDescription(data.description)
    setPlan(PLANS.includes(data.plan as PlanId) ? (data.plan as PlanId) : "free")
  })

  async function handleSubmit(e: Event) {
    e.preventDefault()
    setError("")
    setSubmitting(true)
    try {
      await adminUpdateRelay(relayId(), {
        name: name().trim(),
        subdomain: slugify(subdomain()),
        icon: icon().trim(),
        description: description().trim(),
        plan: plan(),
      })
      navigate(`/admin/relays/${relayId()}`)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update relay")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div class="max-w-2xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href={`/admin/relays/${params.id}`} class="text-gray-500 hover:text-gray-700">← Back</A>
      </div>
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Edit Relay (Admin)</h1>

      <Show when={relay.loading}>
        <p class="text-gray-500">Loading relay...</p>
      </Show>
      <Show when={relay.error && !relay.loading}>
        <p class="text-red-600">Failed to load relay.</p>
      </Show>

      <Show when={relay() && !relay.loading}>
        <form onSubmit={handleSubmit} class="space-y-4">
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
            <input
              value={name()}
              onInput={e => setName(e.currentTarget.value)}
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
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">Plan</label>
            <div class="flex gap-2">
              {PLANS.map(p => (
                <button
                  type="button"
                  class={`rounded-lg border px-3 py-2 text-sm capitalize ${plan() === p ? "border-blue-600 text-blue-700" : "border-gray-300 text-gray-700"}`}
                  onClick={() => setPlan(p)}
                >
                  {p}
                </button>
              ))}
            </div>
          </div>
          <Show when={error()}>
            <p class="text-sm text-red-600">{error()}</p>
          </Show>
          <button
            type="submit"
            disabled={submitting()}
            class="w-full py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50"
          >
            {submitting() ? "Saving..." : "Save Changes"}
          </button>
        </form>
      </Show>
    </div>
  )
}
