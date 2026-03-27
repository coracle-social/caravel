import { useNavigate, useParams } from "@solidjs/router"
import { Show } from "solid-js"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { slugify } from "@/lib/slugify"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/components/useMinLoading"
import { updateRelayById, useRelay } from "@/lib/hooks"

export default function RelayEdit() {
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = useRelay(relayId)
  const loading = useMinLoading(() => relay.loading)

  async function handleSubmit(values: RelayFormValues) {
    await updateRelayById(relayId(), {
      subdomain: slugify(values.subdomain),
      info_name: values.info_name.trim(),
      info_icon: values.info_icon.trim(),
      info_description: values.info_description.trim(),
    })
    navigate(`/relays/${relayId()}`)
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
          initialValues={relay()!}
          onSubmit={handleSubmit}
          submitLabel="Save Changes"
          submittingLabel="Saving..."
        />
      </Show>
    </PageContainer>
  )
}
