import { A } from "@solidjs/router"
import Fuse from "fuse.js"
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import PageContainer from "@/components/PageContainer"
import ResourceState from "@/components/ResourceState"
import SearchInput from "@/components/SearchInput"
import useMinLoading from "@/components/useMinLoading"
import { primeProfiles, useAdminTenants } from "@/lib/hooks"
import { eventStore } from "@/lib/state"

function shortenPubkey(pubkey: string) {
  return pubkey.length <= 16 ? pubkey : `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

export default function AdminTenantList() {
  const [query, setQuery] = createSignal("")
  const [tenants] = useAdminTenants()
  const [profiles, setProfiles] = createSignal<Record<string, { name?: string; about?: string; nip05?: string; picture?: string }>>({})
  const loading = useMinLoading(() => tenants.loading)

  const filtered = createMemo(() => {
    const list = (tenants() ?? []).map((tenant) => {
      const profile = profiles()[tenant.pubkey]
      return { ...tenant, profileName: profile?.name, profileAbout: profile?.about, profileNip05: profile?.nip05 }
    })
    const q = query().trim()
    if (!q) return list
    return new Fuse(list, { keys: ["pubkey", "status", "profileName", "profileAbout", "profileNip05"], threshold: 0.35, ignoreLocation: true }).search(q).map(r => r.item)
  })

  createEffect(() => {
    const list = tenants() ?? []
    if (!list.length) return

    const pubkeys = list.map(t => t.pubkey)
    const reqSub = primeProfiles(pubkeys)
    const profileSubs = pubkeys.map((pubkey) =>
      eventStore.profile(pubkey).subscribe((profile) => {
        setProfiles(prev => ({
          ...prev,
          [pubkey]: {
            name: profile?.name || profile?.display_name,
            about: profile?.about,
            nip05: profile?.nip05,
            picture: getProfilePicture(profile),
          },
        }))
      }),
    )

    onCleanup(() => {
      reqSub.unsubscribe()
      for (const sub of profileSubs) sub.unsubscribe()
    })
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
                return (
                  <li>
                    <A href={`/admin/tenants/${tenant.pubkey}`} class="block border border-gray-200 rounded-lg p-4 bg-white hover:border-gray-300">
                      <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0 flex items-start gap-3">
                          <Show
                            when={profile()?.picture}
                            fallback={<div class="h-10 w-10 rounded-full bg-gray-200 flex items-center justify-center text-gray-500 text-xs">{(profile()?.name || tenant.pubkey).slice(0, 1).toUpperCase()}</div>}
                          >
                            <img src={profile()?.picture} alt="Profile" class="h-10 w-10 rounded-full object-cover" />
                          </Show>
                          <div class="min-w-0">
                            <p class="font-medium text-gray-900 truncate">{profile()?.name || shortenPubkey(tenant.pubkey)}</p>
                            <p class="mt-1 text-sm text-gray-600 line-clamp-2">{profile()?.about || "No profile bio"}</p>
                            <p class="mt-1 text-xs text-gray-500 break-all">{tenant.pubkey}</p>
                          </div>
                        </div>
                        <p class="text-xs uppercase tracking-wide text-gray-500">tenant</p>
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
