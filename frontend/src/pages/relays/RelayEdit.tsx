import { useNavigate, useParams } from "@solidjs/router"
import { Show, createEffect, createSignal } from "solid-js"
import RelayForm from "../../components/RelayForm"
import { slugify } from "../../lib/slugify"
import BackLink from "../../components/BackLink"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"
import useMinLoading from "../../components/useMinLoading"
import { updateRelayById, useRelay } from "../../lib/hooks"

export default function RelayEdit() {
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = useRelay(relayId)
  const loading = useMinLoading(() => relay.loading)

  const [name, setName] = createSignal("")
  const [subdomain, setSubdomain] = createSignal("")
  const [icon, setIcon] = createSignal("")
  const [description, setDescription] = createSignal("")
  const [error, setError] = createSignal("")
  const [submitting, setSubmitting] = createSignal(false)

  createEffect(() => {
    const data = relay()
    if (!data) return
    setName(data.info_name)
    setSubdomain(data.subdomain)
    setIcon(data.info_icon)
    setDescription(data.info_description)
  })

  async function handleSubmit(e: Event) {
    e.preventDefault()
    setError("")
    setSubmitting(true)
    try {
      await updateRelayById(relayId(), {
        subdomain: slugify(subdomain()),
        info_name: name().trim(),
        info_icon: icon().trim(),
        info_description: description().trim(),
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
