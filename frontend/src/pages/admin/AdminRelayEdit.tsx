import { A, useNavigate, useParams } from "@solidjs/router"
import { Show, createEffect, createResource, createSignal } from "solid-js"
import { adminGetRelay, adminUpdateRelay } from "../../lib/api"
import RelayForm from "../../components/RelayForm"
import { RELAY_PLAN_IDS, type RelayPlanId } from "../../lib/relayPlans"
import { slugify } from "../../lib/slugify"

export default function AdminRelayEdit() {
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = createResource(relayId, adminGetRelay)

  const [name, setName] = createSignal("")
  const [subdomain, setSubdomain] = createSignal("")
  const [icon, setIcon] = createSignal("")
  const [description, setDescription] = createSignal("")
  const [plan, setPlan] = createSignal<RelayPlanId>("free")
  const [error, setError] = createSignal("")
  const [submitting, setSubmitting] = createSignal(false)

  createEffect(() => {
    const data = relay()
    if (!data) return
    setName(data.name)
    setSubdomain(data.subdomain)
    setIcon(data.icon)
    setDescription(data.description)
    setPlan(RELAY_PLAN_IDS.includes(data.plan as RelayPlanId) ? (data.plan as RelayPlanId) : "free")
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
        <RelayForm
          name={name()}
          setName={setName}
          subdomain={subdomain()}
          setSubdomain={setSubdomain}
          icon={icon()}
          setIcon={setIcon}
          description={description()}
          setDescription={setDescription}
          plan={plan()}
          setPlan={setPlan}
          plans={RELAY_PLAN_IDS}
          onSubmit={handleSubmit}
          submitting={submitting()}
          error={error()}
          submitLabel="Save Changes"
          submittingLabel="Saving..."
        />
      </Show>
    </div>
  )
}
