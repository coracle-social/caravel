import { createEffect, createResource, For, Show } from "solid-js"
import Card from "@/portal/components/Card"
import Notice from "@/portal/components/Notice"
import Page from "@/portal/components/Page"
import SignIn from "@/portal/components/SignIn"
import { formatLongDate } from "@/shared/lib/format"
import { hasStatus } from "@/shared/lib/http"
import { loaded } from "@/shared/lib/resource"
import { getAccount } from "@/portal/lib/api"
import { shortNpub } from "@/portal/lib/nostr"
import { session, signOut } from "@/portal/lib/session"

const STATUS_LABELS = {
  pending: "Awaiting payment",
  active: "Renews",
  canceled: "Ends",
  lapsed: "Ended",
}

// Every membership the signed-in member holds, each linking to its relay's own page.
export default function AccountPage() {
  const [account] = createResource(() => session() || false, getAccount)

  const current = () => loaded(account)

  // A session that no longer works ends.
  createEffect(() => {
    if (hasStatus(account.error, 401)) signOut()
  })

  return (
    <Page>
      <Card>
        <h1 class="text-xl font-semibold">Your memberships</h1>
        <Show
          when={session()}
          fallback={
            <div class="space-y-3">
              <p class="text-sm text-zinc-600">Sign in with Nostr to see and manage your memberships.</p>
              <SignIn />
            </div>
          }
        >
          <Show when={account.loading && !current()}>
            <p class="text-sm text-zinc-500">Loading…</p>
          </Show>
          <Show when={account.error && !hasStatus(account.error, 401)}>
            <Notice tone="error">We couldn't load your memberships. Try again in a moment.</Notice>
          </Show>
          <Show when={current()}>
            {(a) => (
              <>
                <Show
                  when={a().memberships.length > 0}
                  fallback={<p class="text-sm text-zinc-600">You don't have any memberships yet.</p>}
                >
                  <ul class="divide-y divide-zinc-100">
                    <For each={a().memberships}>
                      {({ offer, membership }) => (
                        <li>
                          <a href={`https://${offer.host}/`} class="flex items-center gap-3 py-3 hover:opacity-80">
                            <Show
                              when={offer.icon}
                              fallback={
                                <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-zinc-100 font-semibold text-zinc-500">
                                  {(offer.name || offer.host).slice(0, 1).toUpperCase()}
                                </div>
                              }
                            >
                              <img src={offer.icon} alt="" class="h-10 w-10 shrink-0 rounded-lg object-cover" />
                            </Show>
                            <div class="min-w-0 flex-1">
                              <p class="truncate text-sm font-medium">{offer.name || offer.host}</p>
                              <p class="text-xs text-zinc-500">
                                {STATUS_LABELS[membership.status]}
                                <Show when={membership.current_period_end && membership.status !== "pending"}>
                                  {" "}{formatLongDate(membership.current_period_end!)}
                                </Show>
                              </p>
                            </div>
                            <span class="text-zinc-400">›</span>
                          </a>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <div class="flex items-center justify-between border-t border-zinc-100 pt-4 text-sm text-zinc-500">
                  <span class="font-mono">{shortNpub(a().pubkey)}</span>
                  <button type="button" onClick={signOut} class="hover:text-zinc-700">Sign out</button>
                </div>
              </>
            )}
          </Show>
        </Show>
      </Card>
    </Page>
  )
}
