import { useNavigate, useParams } from "@solidjs/router"
import { Show, createEffect, createResource, createSignal } from "solid-js"
import { getTenantRelay, updateTenantRelay } from "../../lib/api"
import RelayForm from "../../components/RelayForm"
import { RELAY_PLAN_IDS, type RelayPlanId } from "../../lib/relayPlans"
import { slugify } from "../../lib/slugify"
import BackLink from "../../components/BackLink"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"

export default function RelayEdit() {
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = createResource(relayId, getTenantRelay)
  const loading = useMinLoading(() => relay.loading)

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
      await updateTenantRelay(relayId(), {
        name: name().trim(),
        subdomain: slugify(subdomain()),
        icon: icon().trim(),
        description: description().trim(),
        plan: plan(),
      })
      navigate(`/relays/${relayId()}`)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update relay")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <PageContainer size="narrow">
      <BackLink href={`/relays/${params.id}`} label="Back" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Edit Relay</h1>

      <ResourceState
        loading={loading()}
        error={relay.error}
        loadingText="Loading relay..."
        errorText="Failed to load relay."
      />

      <Show when={relay() && !loading()}>
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
    </PageContainer>
  )
}
