import { A } from "@solidjs/router"
import { createMemo, createResource, createSignal, For, Show } from "solid-js"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import SearchInput from "@/shared/components/SearchInput"
import useMinLoading from "@/shared/lib/useMinLoading"
import { profileName, useProfileMetadataMap } from "@/shared/lib/profiles"
import { listTenants, tenantStatus } from "@/shared/lib/api"
import Avatar from "@/shared/components/Avatar"
import StatusBadge from "@/shared/components/StatusBadge"
import CopyNpub from "@/shared/components/CopyNpub"
import { shortenNpub } from "@/shared/lib/pubkey"
import { loaded } from "@/shared/lib/resource"
import { fuzzySearch } from "@/shared/lib/search"

export default function AdminTenantList() {
  const [query, setQuery] = createSignal("")
  const [tenants] = createResource(listTenants)
  const profiles = useProfileMetadataMap(() => (loaded(tenants) ?? []).map((t) => t.pubkey))
  const loading = useMinLoading(() => tenants.loading)

  const filtered = createMemo(() => {
    const list = (loaded(tenants) ?? []).map((tenant) => {
      const profile = profiles()[tenant.pubkey]
      return {
        ...tenant,
        status: tenantStatus(tenant),
        profileName: profileName(profile),
        profileAbout: profile?.about,
        profileNip05: profile?.nip05,
      }
    })
    return fuzzySearch(list, ["pubkey", "status", "profileName", "profileAbout", "profileNip05"], query().trim())
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Tenants</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search tenants..." />
      </div>
      <ResourceState loading={loading()} error={tenants.error} loadingText="Loading tenants..." errorText="Failed to load tenants." />
      <Show when={!loading() && loaded(tenants)}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No tenants found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(tenant) => {
                const profile = () => profiles()[tenant.pubkey]
                const name = () => profileName(profile())
                return (
                  <li>
                    <A href={`/tenants/${tenant.pubkey}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                      <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0 flex items-start gap-3">
                          <Avatar profile={profile()} pubkey={tenant.pubkey} alt="Profile" class="h-10 w-10" fallbackClass="bg-gray-200 text-gray-500 text-xs" />
                          <div class="min-w-0">
                            <p class="font-medium text-gray-900 truncate">{name() || shortenNpub(tenant.pubkey)}</p>
                            <p class="-mt-1">
                              <CopyNpub pubkey={tenant.pubkey} class="text-xs text-gray-500" />
                            </p>
                            <p class="mt-1 text-sm text-gray-600 line-clamp-2">{profile()?.about || "No profile bio"}</p>
                          </div>
                        </div>
                        <StatusBadge status={tenantStatus(tenant)} />
                      </div>
                    </A>
                  </li>
                )
              }}
            </For>
          </ul>
        </Show>
      </Show>
    </PageContainer>
  )
}
