import { useNavigate } from "@solidjs/router"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { createRelayForActiveTenant } from "@/lib/hooks"

export default function RelayNew() {
  const navigate = useNavigate()

  async function handleSubmit(values: RelayFormValues) {
    const relay = await createRelayForActiveTenant(values)
    navigate(`/relays/${relay.id}`)
  }

  return (
    <PageContainer size="narrow">
      <BackLink href="/relays" label="Relays" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">New Relay</h1>
      <RelayForm
        syncSubdomainWithName
        onSubmit={handleSubmit}
        submitLabel="Create Relay"
        submittingLabel="Creating..."
      />
    </PageContainer>
  )
}
