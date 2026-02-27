import { A } from "@solidjs/router"
import { createEffect, createMemo, createResource, createSignal, For, onCleanup, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { adminListTenants } from "../../lib/api"
import { eventStore, primeProfiles } from "../../lib/nostr"
import PageContainer from "../../components/PageContainer"
import ResourceState from "../../components/ResourceState"

function shortenPubkey(pubkey: string) {
  if (pubkey.length <= 16) return pubkey
  return `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

export default function AdminTenants() {
  const [query, setQuery] = createSignal("")
  const [tenants] = createResource(adminListTenants)
  const [profiles, setProfiles] = createSignal<Record<string, { name?: string, about?: string, picture?: string }>>({})

  const filtered = createMemo(() => {
    const q = query().trim().toLowerCase()
    return (tenants() ?? []).filter((tenant) => {
      if (!q) return true
      return tenant.pubkey.toLowerCase().includes(q) || tenant.status.toLowerCase().includes(q)
    })
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
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Tenants</h1>
      <input
        type="search"
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
        placeholder="Search tenants..."
        class="w-full border border-gray-300 rounded-lg px-3 py-2 mb-6 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />

      <ResourceState
        loading={tenants.loading}
        error={tenants.error}
        loadingText="Loading tenants..."
        errorText="Failed to load tenants."
      />
      <Show when={(filtered().length ?? 0) > 0} fallback={<p class="text-gray-500">No tenants found.</p>}>
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
                    <p class="text-xs uppercase tracking-wide text-gray-500">{tenant.status}</p>
                  </div>
                </A>
              </li>
              )
            }}
          </For>
        </ul>
      </Show>
    </PageContainer>
  )
}
