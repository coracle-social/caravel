import { useParams } from "@solidjs/router"
import { createResource, createSignal, For, Show } from "solid-js"
import BackLink from "@/shared/components/BackLink"
import PageContainer from "@/shared/components/PageContainer"
import RelayListItem from "@/shared/components/RelayListItem"
import AdminInvoiceListItem from "@/admin/components/AdminInvoiceListItem"
import ResourceState from "@/shared/components/ResourceState"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import Avatar from "@/shared/components/Avatar"
import ConfirmDialog, { type Confirmation } from "@/shared/components/ConfirmDialog"
import CopyNpub from "@/shared/components/CopyNpub"
import StatusBadge from "@/shared/components/StatusBadge"
import useMinLoading from "@/shared/lib/useMinLoading"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"
import { banTenant, deleteTenant, getTenant, listTenantInvoices, listTenantRelays, tenantStatus, unbanTenant } from "@/shared/lib/api"
import { formatDateTime, formatError } from "@/shared/lib/format"
import { setToastMessage } from "@/shared/lib/state"
import { shortenNpub } from "@/shared/lib/pubkey"
import { loaded } from "@/shared/lib/resource"

type Action = "delete" | "ban" | "unban"

function confirmation(action: Action, label: string): Confirmation {
  switch (action) {
    case "delete":
      return {
        title: "Delete tenant?",
        description: `${label}'s relays will be deleted and their billing settled.`,
        details: [
          "All of the tenant's relays will be permanently deleted — data and config — and their subdomains freed.",
          "Billing is canceled: open invoices are voided and outstanding charges forgiven.",
          "The tenant is not banned, and can sign in and create relays again.",
          "Deleted relays and forgiven invoices are not restored.",
        ],
        confirmLabel: "Yes, delete tenant",
        busyLabel: "Deleting...",
        tone: "danger",
      }
    case "ban":
      return {
        title: "Ban tenant?",
        description: `${label} will be banned immediately.`,
        details: [
          "All of the tenant's relays will be permanently deleted — data and config.",
          "Billing is canceled: open invoices are voided and outstanding charges forgiven.",
          "The tenant can no longer create relays.",
          "Lifting the ban later does not restore deleted relays.",
        ],
        confirmLabel: "Yes, ban tenant",
        busyLabel: "Banning...",
        tone: "danger",
      }
    case "unban":
      return {
        title: "Unban tenant?",
        description: `${label} will be able to use the service again.`,
        details: [
          "The tenant can create relays again, and billing resumes on new activity.",
          "Deleted relays and forgiven invoices are not restored.",
        ],
        confirmLabel: "Yes, unban tenant",
        busyLabel: "Unbanning...",
        tone: "primary",
      }
  }
}

export default function AdminTenantDetail() {
  const params = useParams()
  const tenantId = () => params.id ?? ""
  const [tenant, { refetch: refetchTenant }] = createResource(tenantId, getTenant)
  const [relays, { refetch: refetchRelays }] = createResource(tenantId, listTenantRelays)
  const [invoices, { refetch: refetchInvoices }] = createResource(tenantId, listTenantInvoices)
  const loading = useMinLoading(() => tenant.loading || relays.loading || invoices.loading)
  const failed = () => tenant.error || relays.error || invoices.error
  const metadata = useProfileMetadata(tenantId)

  const [pendingAction, setPendingAction] = createSignal<Action>()
  const [busy, setBusy] = createSignal(false)

  const tenantLabel = () => profileName(metadata()) || shortenNpub(tenantId())

  async function confirmAction() {
    const action = pendingAction()
    if (!action || busy()) return

    setBusy(true)
    try {
      if (action === "delete") {
        await deleteTenant(tenantId())
        setToastMessage("Tenant deleted", "success")
      } else if (action === "ban") {
        await banTenant(tenantId())
        setToastMessage("Tenant banned", "success")
      } else {
        await unbanTenant(tenantId())
        setToastMessage("Tenant unbanned", "success")
      }
      // Deleting and banning delete the tenant's relays and void its open invoices.
      await Promise.all([refetchTenant(), refetchRelays(), refetchInvoices()])
      setPendingAction(undefined)
    } catch (e) {
      setToastMessage(formatError(e, `Failed to ${action} tenant`))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageContainer>
      <BackLink label="Back" />
      <div class="flex items-start justify-between gap-4 mb-6 py-2">
        <div class="flex items-center gap-4 min-w-0">
          <Avatar profile={metadata()} pubkey={tenantId()} alt="Profile" class="h-14 w-14 flex-shrink-0" fallbackClass="bg-gray-200 text-gray-500 text-lg" />
          <div class="min-w-0">
            <h1 class="text-2xl font-bold text-gray-900 truncate">{tenantLabel()}</h1>
            <p class="mt-0.5">
              <CopyNpub pubkey={tenantId()} class="text-xs text-gray-500" />
            </p>
          </div>
        </div>
        <Show when={loaded(tenant)}>
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
                      setPendingAction("delete")
                    }}
                  >
                    Delete tenant
                  </ActionsMenuItem>
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
      <ResourceState loading={loading()} error={failed()} loadingText="Loading tenant..." errorText="Failed to load tenant." class="mb-4" />
      <Show when={!loading() && !failed()}>
        <div class="space-y-6">
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Status</h2>
            <Show when={loaded(tenant)}>
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
                  <Show when={t().banned_at}>
                    {(at) => (
                      <div class="flex gap-2">
                        <dt class="text-gray-500">Banned Since:</dt>
                        <dd class="text-red-600 font-medium">{formatDateTime(at())}</dd>
                      </div>
                    )}
                  </Show>
                  <Show when={t().churned_at}>
                    {(at) => (
                      <div class="flex gap-2">
                        <dt class="text-gray-500">Delinquent Since:</dt>
                        <dd class="text-red-600 font-medium">{formatDateTime(at())}</dd>
                      </div>
                    )}
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
            <Show when={(loaded(relays)?.length ?? 0) > 0} fallback={<p class="text-gray-500">No relays.</p>}>
              <ul class="space-y-3">
                <For each={loaded(relays) ?? []}>
                  {(relay) => <RelayListItem relay={relay} href={`/relays/${relay.id}`} />}
                </For>
              </ul>
            </Show>
          </section>
          <section class="bg-white border border-gray-200 rounded-xl p-6">
            <h2 class="text-lg font-semibold mb-4">Invoices</h2>
            <Show when={(loaded(invoices)?.length ?? 0) > 0} fallback={<p class="text-gray-500">No invoices.</p>}>
              <ul class="space-y-3">
                <For each={loaded(invoices) ?? []}>
                  {(invoice) => <AdminInvoiceListItem invoice={invoice} href={`/invoices/${invoice.id}`} />}
                </For>
              </ul>
            </Show>
          </section>
        </div>
      </Show>
      <ConfirmDialog
        confirmation={pendingAction() && confirmation(pendingAction()!, tenantLabel())}
        busy={busy()}
        onConfirm={confirmAction}
        onClose={() => {
          if (!busy()) setPendingAction(undefined)
        }}
      />
    </PageContainer>
  )
}
