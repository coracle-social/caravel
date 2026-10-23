import { useParams } from "@solidjs/router"
import { createResource, Show } from "solid-js"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import { listRelayMembers } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import useMinLoading from "@/shared/lib/useMinLoading"
import { useFramedRelay } from "@/tenant/components/RelayFrame"
import InviteCodes from "@/tenant/components/relay/InviteCodes"
import MemberList from "@/tenant/components/relay/MemberList"
import RoleList from "@/tenant/components/relay/RoleList"
import StaffList from "@/tenant/components/relay/StaffList"

// Who belongs to a relay and how: its members and their roles, invite codes, and staff.
export default function RelayMembers() {
  const params = useParams()
  const [relay] = useFramedRelay()
  const [access, { refetch }] = createResource(() => params.id, listRelayMembers)
  const loading = useMinLoading(() => access.loading && !access())

  return (
    <PageContainer>
      <ResourceState loading={loading()} error={access.error} loadingText="Loading members..." errorText="Failed to load members." class="mb-4" />
      <Show when={!loading() && loaded(access)}>
        {(current) => (
          <div class="space-y-6">
            <MemberList relayId={params.id!} members={current().members} roles={current().roles} onChanged={() => void refetch()} />
            <RoleList relayId={params.id!} roles={current().roles} onChanged={() => void refetch()} />
            <Show when={loaded(relay)}>
              {(r) => (
                <>
                  <InviteCodes relay={r()} />
                  <StaffList relay={r()} />
                </>
              )}
            </Show>
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
