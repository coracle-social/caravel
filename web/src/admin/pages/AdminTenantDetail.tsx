import { useParams } from "@solidjs/router"
import { createSignal, For, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import BackLink from "@/shared/components/BackLink"
import PageContainer from "@/shared/components/PageContainer"
import RelayListItem from "@/shared/components/RelayListItem"
import AdminInvoiceListItem from "@/admin/components/AdminInvoiceListItem"
import ResourceState from "@/shared/components/ResourceState"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import { StatusBadge } from "@/shared/components/relay/RelayCardHeader"
import useMinLoading from "@/shared/lib/useMinLoading"
import { useAdminTenant, useAdminTenantInvoices, useAdminTenantRelays, useProfileMetadata } from "@/shared/lib/hooks"
import { banTenant, tenantStatus, unbanTenant } from "@/shared/lib/api"
import { setToastMessage } from "@/shared/lib/state"
import CopyNpub from "@/shared/components/CopyNpub"
import { shortenNpub } from "@/shared/lib/pubkey"

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [tenant, { refetch: refetchTenant }] = useAdminTenant(tenantId)
  const [relays, { refetch: refetchRelays }] = useAdminTenantRelays(tenantId)
  const [invoices, { refetch: refetchInvoices }] = useAdminTenantInvoices(tenantId)
  const loading = useMinLoading(() => tenant.loading || relays.loading || invoices.loading)
  const metadata = useProfileMetadata(tenantId)

  const [pendingAction, setPendingAction] = createSignal<"ban" | "unban" | null>(null)
  const [busy, setBusy] = createSignal(false)

  const tenantLabel = () => (metadata()?.name || metadata()?.display_name) || shortenNpub(tenantId())

  const bannedLabel = () => {
    const ts = tenant()?.banned_at
    if (!ts) return null
    return new Date(ts * 1000).toLocaleString()
  }

  const churnedLabel = () => {
    const ts = tenant()?.churned_at
    if (!ts) return null
    return new Date(ts * 1000).toLocaleString()
  }

  async function confirmAction() {
    const action = pendingAction()
    if (!action || busy()) return

    setBusy(true)
    try {
      if (action === "ban") {
        await banTenant(tenantId())
        setToastMessage("Tenant banned", "success")
      } else {
        await unbanTenant(tenantId())
        setToastMessage("Tenant unbanned", "success")
      }
      // Banning purges the tenant's relays and voids its open invoices.
      await Promise.all([refetchTenant(), refetchRelays(), refetchInvoices()])
      setPendingAction(null)
    } catch (e) {
      setToastMessage(e instanceof Error ? e.message : `Failed to ${action} tenant`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageContainer>
      <BackLink label="Back" />
      <div class="flex items-start justify-between gap-4 mb-6 py-2">
        <div class="flex items-center gap-4 min-w-0">
          <Show
            when={getProfilePicture(metadata())}
            fallback={
              <div class="h-14 w-14 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-gray-500 text-lg">
                {((metadata()?.name || metadata()?.display_name) || tenantId()).slice(0, 1).toUpperCase()}
              </div>
            }
          >
            <img src={getProfilePicture(metadata())} alt="Profile" class="h-14 w-14 flex-shrink-0 rounded-full object-cover" />
          </Show>
          <div class="min-w-0">
            <h1 class="text-2xl font-bold text-gray-900 truncate">{tenantLabel()}</h1>
            <p class="mt-0.5">
              <CopyNpub pubkey={tenantId()} class="text-xs text-gray-500" />
            </p>
          </div>
        </div>
        <Show when={tenant()}>
          {(t) => (
            <ActionsMenu>
              {(close) => (
                <Show
                  when={t().banned_at == null}
                  fallback={
                    <ActionsMenuItem
                      tone="primary"
                      onClick={() => {
                        close()
                        setPendingAction("unban")
                      }}
                    >
                      Unban tenant
                    </ActionsMenuItem>
                  }
                >
                  <ActionsMenuItem
                    tone="danger"
                    onClick={() => {
                      close()
                      setPendingAction("ban")
                    }}
                  >
                    Ban tenant
                  </ActionsMenuItem>
                </Show>
              )}
            </ActionsMenu>
          )}
        </Show>
      </div>
      <ResourceState loading={loading()} error={tenant.error || relays.error || invoices.error} loadingText="Loading tenant..." errorText="Failed to load tenant." class="mb-4" />
      <Show when={!loading()}>
        <div class="space-y-6">
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Status</h2>
            <Show when={tenant()}>
              {(t) => (
                <dl class="grid gap-y-3 text-sm">
                  <div class="flex gap-2 items-center">
                    <dt class="text-gray-500">Status:</dt>
                    <dd>
                      <StatusBadge status={tenantStatus(t())} />
                    </dd>
                  </div>
                  <Show when={t().stripe_customer_id}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Stripe Customer:</dt>
                      <dd class="font-mono">{t().stripe_customer_id}</dd>
                    </div>
                  </Show>
                  <Show when={bannedLabel()}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Banned Since:</dt>
                      <dd class="text-red-600 font-medium">{bannedLabel()}</dd>
                    </div>
                  </Show>
                  <Show when={churnedLabel()}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Delinquent Since:</dt>
                      <dd class="text-red-600 font-medium">{churnedLabel()}</dd>
                    </div>
                  </Show>
                  <Show when={t().nwc_error}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">NWC Error:</dt>
                      <dd class="text-red-600">{t().nwc_error}</dd>
                    </div>
                  </Show>
                  <Show when={t().stripe_error}>
                    <div class="flex gap-2">
                      <dt class="text-gray-500">Stripe Error:</dt>
                      <dd class="text-red-600">{t().stripe_error}</dd>
                    </div>
                  </Show>
                </dl>
              )}
            </Show>
          </section>
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Relays</h2>
            <Show when={(relays()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No relays.</p>}>
              <ul class="space-y-3">
                <For each={relays() ?? []}>
                  {(relay) => <RelayListItem relay={relay} href={`/admin/relays/${relay.id}`} />}
                </For>
              </ul>
            </Show>
          </section>
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Invoices</h2>
            <Show when={(invoices()?.length ?? 0) > 0} fallback={<p class="text-gray-500">No invoices.</p>}>
              <ul class="space-y-3">
                <For each={invoices() ?? []}>
                  {(invoice) => <AdminInvoiceListItem invoice={invoice} href={`/admin/invoices/${invoice.id}`} />}
                </For>
              </ul>
            </Show>
          </section>
        </div>
      </Show>
      <ConfirmDialog
        open={pendingAction() !== null}
        title={pendingAction() === "ban" ? "Ban tenant?" : "Unban tenant?"}
        description={
          pendingAction() === "ban"
            ? `${tenantLabel()} will be banned immediately.`
            : `${tenantLabel()} will be able to use the service again.`
        }
        details={
          pendingAction() === "ban"
            ? [
                "All of the tenant's relays will be permanently purged — data and config deleted.",
                "Billing is canceled: open invoices are voided and outstanding charges forgiven.",
                "The tenant can no longer create relays.",
                "Lifting the ban later does not restore purged relays.",
              ]
            : [
                "The tenant can create relays again, and billing resumes on new activity.",
                "Purged relays and forgiven invoices are not restored.",
              ]
        }
        confirmLabel={pendingAction() === "ban" ? "Yes, ban tenant" : "Yes, unban tenant"}
        busyLabel={pendingAction() === "ban" ? "Banning..." : "Unbanning..."}
        busy={busy()}
        tone={pendingAction() === "ban" ? "danger" : "primary"}
        onConfirm={confirmAction}
        onClose={() => {
          if (!busy()) setPendingAction(null)
        }}
      />
    </PageContainer>
  )
}
