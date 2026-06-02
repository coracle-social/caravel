import { A } from "@solidjs/router"
import { createMemo, createSignal, For, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import PageContainer from "@/components/PageContainer"
import ResourceState from "@/components/ResourceState"
import SearchInput from "@/components/SearchInput"
import useMinLoading from "@/lib/useMinLoading"
import { useAdminTenants, useProfileMetadataMap } from "@/lib/hooks"
import { shortenPubkey } from "@/lib/pubkey"
import { fuzzySearch } from "@/lib/search"

export default function AdminTenantList() {
  const [query, setQuery] = createSignal("")
  const [tenants] = useAdminTenants()
  const profiles = useProfileMetadataMap(() => (tenants() ?? []).map((t) => t.pubkey))
  const loading = useMinLoading(() => tenants.loading)

  const filtered = createMemo(() => {
    const list = (tenants() ?? []).map((tenant) => {
      const profile = profiles()[tenant.pubkey]
      return {
        ...tenant,
        profileName: profile?.name || profile?.display_name,
        profileAbout: profile?.about,
        profileNip05: profile?.nip05,
      }
    })
    const q = query().trim()
    if (!q) return list
    return fuzzySearch(list, ["pubkey", "status", "profileName", "profileAbout", "profileNip05"], q)
  })

  return (
    <PageContainer>
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">Tenants</h1>
      <div class="mb-6">
        <SearchInput value={query()} onInput={setQuery} placeholder="Search tenants..." />
      </div>
      <ResourceState loading={loading()} error={tenants.error} loadingText="Loading tenants..." errorText="Failed to load tenants." />
      <Show when={!loading()}>
        <Show when={filtered().length > 0} fallback={<p class="py-20 text-center text-gray-500">No tenants found.</p>}>
          <ul class="space-y-3">
            <For each={filtered()}>
              {(tenant) => {
                const profile = () => profiles()[tenant.pubkey]
                const profileName = () => profile()?.name || profile()?.display_name
                return (
                  <li>
                    <A href={`/admin/tenants/${tenant.pubkey}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                      <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0 flex items-start gap-3">
                          <Show
                            when={getProfilePicture(profile())}
                            fallback={<div class="h-10 w-10 rounded-full bg-gray-200 flex items-center justify-center text-gray-500 text-xs">{(profileName() || tenant.pubkey).slice(0, 1).toUpperCase()}</div>}
                          >
                            <img src={getProfilePicture(profile())} alt="Profile" class="h-10 w-10 rounded-full object-cover" />
                          </Show>
                          <div class="min-w-0">
                            <p class="font-medium text-gray-900 truncate">{profileName() || shortenPubkey(tenant.pubkey)}</p>
                            <p class="mt-1 text-sm text-gray-600 line-clamp-2">{profile()?.about || "No profile bio"}</p>
                            <p class="mt-1 text-xs text-gray-500 break-all">{tenant.pubkey}</p>
                          </div>
                        </div>
                        <span class={`inline-flex flex-shrink-0 items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${tenant.churned_at ? "bg-red-50 text-red-700 border-red-200" : "bg-green-50 text-green-700 border-green-200"}`}>
                          {tenant.churned_at ? "delinquent" : "active"}
                        </span>
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
