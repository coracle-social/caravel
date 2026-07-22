import { useNavigate, useParams } from "@solidjs/router"
import { Show } from "solid-js"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import ResourceState from "@/components/ResourceState"
import useMinLoading from "@/lib/useMinLoading"
import { updateRelayById, useRelay } from "@/lib/hooks"

export default function RelayEdit(props: { basePath?: string; title?: string }) {
  const basePath = () => props.basePath ?? "/relays"
  const navigate = useNavigate()
  const params = useParams()
  const relayId = () => params.id ?? ""
  const [relay] = useRelay(relayId)
  const loading = useMinLoading(() => relay.loading)

  async function handleSubmit(values: RelayFormValues) {
    await updateRelayById(relayId(), {
      subdomain: values.subdomain,
      info_name: values.info_name.trim(),
      info_icon: values.info_icon.trim(),
      info_description: values.info_description.trim(),
    })
    navigate(`${basePath()}/${relayId()}`)
  }

  return (
    <PageContainer size="narrow">
      <BackLink href={`${basePath()}/${params.id}`} label="Back" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">{props.title ?? "Edit Relay"}</h1>
      <ResourceState loading={loading()} error={relay.error} loadingText="Loading relay..." errorText="Failed to load relay." />
      <Show when={relay() && !loading()}>
        <RelayForm
          initialValues={relay()!}
          zooidDomain={relay()!.zooid_domain}
          showPlanSelector={false}
          onSubmit={handleSubmit}
          submitLabel="Save Changes"
          submittingLabel="Saving..."
        />
      </Show>
    </PageContainer>
  )
}
