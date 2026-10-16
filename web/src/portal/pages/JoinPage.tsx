import { A } from "@solidjs/router"
import { createResource, createSignal, Show } from "solid-js"
import Card from "@/portal/components/Card"
import Notice from "@/portal/components/Notice"
import Page from "@/portal/components/Page"
import RelayHeader from "@/portal/components/RelayHeader"
import { ApiError, createInvite, getOffer } from "@/portal/lib/api"
import { useParam } from "@/portal/lib/params"
import { portalRelay } from "@/portal/lib/relay"

// Buying a membership as someone new to Nostr. They pay first and get an invite
// link, which creates their account and joins the relay in Flotilla.
export default function JoinPage() {
  const [param] = useParam()
  const relay = portalRelay()
  const returnUrl = () => param("return_url")

  const [offer] = createResource(() => relay?.id ?? false, getOffer)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal("")

  const sells = () => (offer.error ? undefined : offer())
  const notFound = () => !relay || (offer.error instanceof ApiError && offer.error.status === 404)

  async function buy() {
    if (!relay) return
    setBusy(true)
    setError("")
    try {
      window.location.href = (await createInvite(relay.id, returnUrl())).url
    } catch (e) {
      setError(e instanceof Error ? e.message : "We couldn't start checkout")
      setBusy(false)
    }
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
                onClick={() => void buy()}
                disabled={busy()}
                class="w-full rounded-xl bg-indigo-600 px-4 py-3 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
              >
                {busy() ? "Opening checkout…" : "Continue to payment"}
              </button>
            </Show>
            <Show when={error()}>
              <Notice tone="error">{error()}</Notice>
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
