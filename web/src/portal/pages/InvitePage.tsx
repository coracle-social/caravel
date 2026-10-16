import { A, useParams } from "@solidjs/router"
import { createResource, createSignal, onMount, Show } from "solid-js"
import Card from "@/portal/components/Card"
import Notice from "@/portal/components/Notice"
import Page from "@/portal/components/Page"
import RelayHeader from "@/portal/components/RelayHeader"
import { formatError } from "@/shared/lib/format"
import { hasStatus } from "@/shared/lib/http"
import { loaded } from "@/shared/lib/resource"
import { getInvite, reconcileInvite } from "@/portal/lib/api"
import { useParam } from "@/portal/lib/params"

// Where a buyer lands after paying for an invite, which hands them its link.
export default function InvitePage() {
  const params = useParams()
  const [param, setSearch] = useParam()
  const code = () => params.code ?? ""

  const [invite, { refetch }] = createResource(code, getInvite)
  const [settling, setSettling] = createSignal(false)
  const [copied, setCopied] = createSignal(false)
  const [error, setError] = createSignal("")

  const current = () => loaded(invite)
  const notFound = () => hasStatus(invite.error, 404)

  // Stripe sends the buyer back with the checkout session they paid in.
  onMount(async () => {
    const checkout = param("checkout")
    if (!checkout) return

    setSettling(true)
    try {
      const { paid } = await reconcileInvite(code(), checkout)
      if (paid) setSearch({ checkout: undefined }, { replace: true })
      await refetch()
    } catch (e) {
      setError(formatError(e, "We couldn't confirm your payment"))
    } finally {
      setSettling(false)
    }
  })

  async function copy(link: string) {
    try {
      await navigator.clipboard.writeText(link)
      setCopied(true)
    } catch {
      setError("Couldn't copy the link. Select it and copy it instead.")
    }
  }

  return (
    <Page>
      <Show when={invite.loading && !current()}>
        <p class="text-center text-sm text-zinc-500">Loading…</p>
      </Show>
      <Show when={invite.error}>
        <Notice tone={notFound() ? "neutral" : "error"}>
          {notFound() ? "We couldn't find this invite." : "We couldn't load this invite. Try again in a moment."}
        </Notice>
      </Show>
      <Show when={current()}>
        {(i) => (
          <Card>
            <RelayHeader relay={i().offer} />

            <Show when={i().status === "pending"}>
              <Notice tone="neutral">
                <p>{settling() ? "Confirming your payment…" : "We haven't seen your payment yet."}</p>
                <Show when={!settling()}>
                  <button type="button" onClick={() => void refetch()} class="font-medium underline">
                    Check again
                  </button>
                </Show>
              </Notice>
            </Show>

            <Show when={i().invite_url}>
              {(link) => (
                <div class="space-y-4">
                  <Notice tone="success">
                    <p class="font-medium">Your invite is ready.</p>
                    <p>Open it to create a free account in Flotilla and join. Whoever uses it first becomes the member, so keep it to yourself.</p>
                  </Notice>
                  <a
                    href={link()}
                    class="block w-full rounded-xl bg-indigo-600 px-4 py-3 text-center text-sm font-medium text-white hover:bg-indigo-500"
                  >
                    Accept your invite
                  </a>
                  <div class="flex gap-2">
                    <input
                      readonly
                      value={link()}
                      onFocus={(e) => e.currentTarget.select()}
                      class="min-w-0 flex-1 rounded-xl border border-zinc-300 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-600"
                    />
                    <button
                      type="button"
                      onClick={() => void copy(link())}
                      class="rounded-xl border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50"
                    >
                      {copied() ? "Copied" : "Copy"}
                    </button>
                  </div>
                </div>
              )}
            </Show>

            <Show when={i().bound}>
              <Notice tone="neutral">
                <p>This invite has been accepted.</p>
                <A href="/" class="font-medium underline">
                  Manage the membership
                </A>
              </Notice>
            </Show>

            <Show when={i().status === "lapsed" && !i().bound}>
              <Notice tone="neutral">This invite has expired.</Notice>
            </Show>

            <Show when={error()}>
              <Notice tone="error">{error()}</Notice>
            </Show>
          </Card>
        )}
      </Show>
    </Page>
  )
}
