import { A } from "@solidjs/router"
import { createResource, Show } from "solid-js"
import Card from "@/portal/components/Card"
import Notice from "@/portal/components/Notice"
import Page from "@/portal/components/Page"
import RelayHeader from "@/portal/components/RelayHeader"
import { hasStatus } from "@/shared/lib/http"
import { loaded } from "@/shared/lib/resource"
import { useRedirect } from "@/shared/lib/useRedirect"
import { createInvite, getOffer } from "@/portal/lib/api"
import { useParam } from "@/portal/lib/params"
import { portalRelay } from "@/portal/lib/relay"

// Buying a membership without a Nostr account, for an invite that makes one in Flotilla.
export default function JoinPage() {
  const [param] = useParam()
  const relay = portalRelay()
  const returnUrl = () => param("return_url")

  const [offer] = createResource(() => relay?.relay_id ?? false, getOffer)
  const checkout = useRedirect()

  const sells = () => loaded(offer)
  const notFound = () => !relay || hasStatus(offer.error, 404)

  function buy() {
    if (!relay) return
    void checkout.go(() => createInvite(relay.relay_id, returnUrl()), "We couldn't start checkout")
  }

  return (
    <Page>
      <Show when={offer.loading}>
        <p class="text-center text-sm text-zinc-500">Loading…</p>
      </Show>
      <Show when={notFound() || offer.error}>
        <Notice tone={notFound() ? "neutral" : "error"}>
          {notFound() ? "This relay doesn't sell memberships." : "We couldn't load this relay. Try again in a moment."}
        </Notice>
      </Show>
      <Show when={sells()}>
        {(o) => (
          <Card>
            <RelayHeader relay={o()} />
            <ol class="space-y-2 text-sm text-zinc-600">
              <li>1. Pay for your membership.</li>
              <li>2. Open your invite link, and create a free account in Flotilla.</li>
              <li>3. You're in. Your membership renews each {o().interval} until you cancel.</li>
            </ol>
            <Show
              when={o().accepting_payment}
              fallback={<Notice tone="neutral">This relay isn't taking payments right now.</Notice>}
            >
              <button
                type="button"
                onClick={buy}
                disabled={checkout.redirecting()}
                class="w-full rounded-xl bg-indigo-600 px-4 py-3 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
              >
                {checkout.redirecting() ? "Opening checkout…" : "Continue to payment"}
              </button>
            </Show>
            <Show when={checkout.error()}>
              <Notice tone="error">{checkout.error()}</Notice>
            </Show>
            <div class="border-t border-zinc-100 pt-6 text-center text-sm text-zinc-600">
              Already use Nostr?{" "}
              <A
                href={`/${returnUrl() ? `?return_url=${encodeURIComponent(returnUrl()!)}` : ""}`}
                class="font-medium text-indigo-600 hover:text-indigo-500"
              >
                Sign in instead
              </A>
            </div>
          </Card>
        )}
      </Show>
    </Page>
  )
}
