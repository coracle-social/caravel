import { A } from "@solidjs/router"
import { createEffect, createResource, createSignal, onMount, Show } from "solid-js"
import Card from "@/portal/components/Card"
import Notice from "@/portal/components/Notice"
import Page from "@/portal/components/Page"
import RelayHeader from "@/portal/components/RelayHeader"
import SignIn from "@/portal/components/SignIn"
import {
  cancelRenewal,
  createCheckout,
  getIdentity,
  getMembership,
  getOffer,
  reconcileCheckout,
} from "@/portal/lib/api"
import { formatError, formatLongDate, formatPrice } from "@/shared/lib/format"
import { hasStatus } from "@/shared/lib/http"
import { loaded } from "@/shared/lib/resource"
import { shortNpub } from "@/portal/lib/nostr"
import { returnLink, useParam } from "@/portal/lib/params"
import { portalRelay } from "@/portal/lib/relay"
import { session, signIn, signOut } from "@/portal/lib/session"
import { FLOTILLA_URL } from "@/shared/lib/config"

// A relay's page on its own host, its NIP-11 payments_url: where a relay that
// sells memberships sells and manages them, and any other says what it is.
export default function RelayPage() {
  const [param, setSearch] = useParam()
  const relay = portalRelay()

  const [offer] = createResource(() => relay?.relay_id ?? false, getOffer)
  const [current, { refetch }] = createResource(
    () => (relay && session() ? { id: relay.relay_id, session: session() } : false),
    ({ id }) => getMembership(id),
  )
  const [identity] = createResource(() => session() || false, getIdentity)
  const [busy, setBusy] = createSignal(false)
  const [confirming, setConfirming] = createSignal(false)
  const [settling, setSettling] = createSignal(false)
  const [unsettled, setUnsettled] = createSignal(false)
  const [error, setError] = createSignal("")

  const sells = () => loaded(offer)
  const free = () => hasStatus(offer.error, 404)
  const membership = () => loaded(current)?.membership
  const pubkey = () => loaded(identity)?.pubkey
  const status = () => membership()?.status
  const back = () => returnLink(param("return_url"))
  const joinHref = () => {
    const value = param("return_url")
    return `/join${value ? `?return_url=${encodeURIComponent(value)}` : ""}`
  }

  // A session that no longer works ends.
  createEffect(() => {
    if (hasStatus(current.error, 401)) signOut()
  })

  onMount(async () => {
    // A notification's link carries a session, which is adopted and kept out of the address bar.
    const token = param("token")
    if (token) {
      signIn(token)
      setSearch({ token: undefined }, { replace: true })
    }

    // Stripe sends the member back with the checkout session they paid in.
    const checkout = param("checkout")
    if (!relay || !checkout || !session()) return

    setSettling(true)
    try {
      const { paid } = await reconcileCheckout(relay.relay_id, checkout)
      setUnsettled(!paid)
      if (paid) setSearch({ checkout: undefined }, { replace: true })
      await refetch()
    } catch (e) {
      setError(formatError(e, "We couldn't confirm your payment"))
    } finally {
      setSettling(false)
    }
  })

  async function checkout() {
    if (!relay) return
    setBusy(true)
    setError("")
    try {
      window.location.href = (await createCheckout(relay.relay_id, param("return_url"))).url
    } catch (e) {
      setError(formatError(e, "We couldn't start checkout"))
      setBusy(false)
    }
  }

  async function cancel() {
    if (!relay) return
    setBusy(true)
    setError("")
    try {
      await cancelRenewal(relay.relay_id)
      setConfirming(false)
      await refetch()
    } catch (e) {
      setError(formatError(e, "We couldn't cancel your renewal"))
    } finally {
      setBusy(false)
    }
  }

  const primary = "w-full rounded-xl bg-indigo-600 px-4 py-3 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"

  return (
    <Page>
      <Show when={relay} fallback={<Notice tone="neutral">There's nothing here.</Notice>}>
        {(r) => (
          <>
            <Show when={offer.loading}>
              <p class="text-center text-sm text-zinc-500">Loading…</p>
            </Show>
            <Show when={offer.error && !free()}>
              <Notice tone="error">We couldn't load this relay. Try again in a moment.</Notice>
            </Show>
            <Show when={free()}>
              <Card>
                <RelayHeader relay={r()} />
                <p class="text-sm text-zinc-600">This is a Nostr relay. Open it in a Nostr client to join.</p>
                <a href={`${FLOTILLA_URL}/join?r=${encodeURIComponent(r().host)}`} class={`${primary} block text-center`}>
                  Open in Flotilla
                </a>
              </Card>
            </Show>
            <Show when={sells()}>
              {(o) => (
                <Card>
                  <RelayHeader relay={o()} />

                  <Show when={settling()}>
                    <Notice tone="neutral">Confirming your payment…</Notice>
                  </Show>
                  <Show when={unsettled()}>
                    <Notice tone="warning">Your payment hasn't gone through yet. Refresh this page in a minute.</Notice>
                  </Show>

                  <Show
                    when={session()}
                    fallback={
                      <div class="space-y-6">
                        <div class="space-y-3">
                          <p class="text-sm text-zinc-600">Sign in with Nostr to join, or to manage a membership you already have.</p>
                          <SignIn />
                        </div>
                        <div class="border-t border-zinc-100 pt-6 text-center text-sm text-zinc-600">
                          New to Nostr?{" "}
                          <A href={joinHref()} class="font-medium text-indigo-600 hover:text-indigo-500">
                            Get an invite instead
                          </A>
                        </div>
                      </div>
                    }
                  >
                    <Show when={!current.loading || current()} fallback={<p class="text-sm text-zinc-500">Loading your membership…</p>}>
                      <Show when={current.error && !hasStatus(current.error, 401)}>
                        <Notice tone="error">We couldn't load your membership. Try again in a moment.</Notice>
                      </Show>

                      <Show when={status() === "active"}>
                        <Notice tone="success">
                          <p class="font-medium">You're a member.</p>
                          <Show when={membership()?.current_period_end}>
                            {(end) => <p>Your membership renews on {formatLongDate(end())}.</p>}
                          </Show>
                        </Notice>
                        <Show
                          when={confirming()}
                          fallback={
                            <button type="button" onClick={() => setConfirming(true)} class="text-sm text-zinc-500 hover:text-zinc-700">
                              Cancel renewal
                            </button>
                          }
                        >
                          <div class="space-y-3 rounded-xl border border-zinc-200 p-4">
                            <p class="text-sm text-zinc-700">
                              You'll keep access until your current period ends, and you won't be charged again.
                            </p>
                            <div class="flex gap-2">
                              <button
                                type="button"
                                onClick={() => void cancel()}
                                disabled={busy()}
                                class="flex-1 rounded-xl bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-500 disabled:opacity-50"
                              >
                                {busy() ? "Canceling…" : "Cancel renewal"}
                              </button>
                              <button
                                type="button"
                                onClick={() => setConfirming(false)}
                                class="flex-1 rounded-xl border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50"
                              >
                                Keep it
                              </button>
                            </div>
                          </div>
                        </Show>
                      </Show>

                      <Show when={status() === "canceled"}>
                        <Notice tone="warning">
                          <p class="font-medium">Your membership won't renew.</p>
                          <Show when={membership()?.current_period_end}>
                            {(end) => <p>You have access until {formatLongDate(end())}. Renew now and you won't be charged until then.</p>}
                          </Show>
                        </Notice>
                      </Show>

                      <Show when={loaded(current) && status() !== "active"}>
                        <Show
                          when={o().accepting_payment}
                          fallback={<Notice tone="neutral">This relay isn't taking payments right now.</Notice>}
                        >
                          <button type="button" onClick={() => void checkout()} disabled={busy() || settling()} class={primary}>
                            {busy() ? "Opening checkout…" : `${status() === "canceled" ? "Renew" : "Join"} for ${formatPrice(o().price, o().interval)}`}
                          </button>
                        </Show>
                      </Show>

                      <Show when={status() === "active" && back()}>
                        {(url) => (
                          <a href={url().href} class={`${primary} block text-center`}>
                            Back to {url().host}
                          </a>
                        )}
                      </Show>

                      <div class="flex items-center justify-between border-t border-zinc-100 pt-4 text-sm text-zinc-500">
                        <Show when={pubkey()}>{(pk) => <span class="font-mono">{shortNpub(pk())}</span>}</Show>
                        <div class="flex gap-4">
                          <A href="/account" class="hover:text-zinc-700">Your memberships</A>
                          <button type="button" onClick={signOut} class="hover:text-zinc-700">Sign out</button>
                        </div>
                      </div>
                    </Show>
                  </Show>

                  <Show when={error()}>
                    <Notice tone="error">{error()}</Notice>
                  </Show>
                </Card>
              )}
            </Show>
          </>
        )}
      </Show>
    </Page>
  )
}
