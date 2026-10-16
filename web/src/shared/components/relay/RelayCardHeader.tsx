import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { getProfilePicture, type ProfileContent } from "applesauce-core/helpers/profile"
import { relayStatus, type Relay } from "@/shared/lib/api"
import { FREE } from "@/shared/lib/plans"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import CopyNpub from "@/shared/components/CopyNpub"

const STATUS_STYLES: Record<string, string> = {
  active:     "bg-green-50 text-green-700 border-green-200",
  inactive:   "bg-gray-100 text-gray-500 border-gray-200",
  verified:   "bg-green-50 text-green-700 border-green-200",
  delinquent: "bg-red-50 text-red-700 border-red-200",
  purged:     "bg-red-50 text-red-700 border-red-200",
  banned:     "bg-gray-800 text-gray-100 border-gray-800",
  canceled:   "bg-yellow-50 text-yellow-700 border-yellow-200",
}

export function StatusBadge(props: { status: string }) {
  const styles = () => STATUS_STYLES[props.status] ?? "bg-gray-100 text-gray-500 border-gray-200"
  const label = () => props.status.replace(/_/g, " ")
  return (
    <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${styles()}`}>
      {label()}
    </span>
  )
}

export function PlanBadge(props: { planId: string }) {
  const styles = () => props.planId === FREE
    ? "bg-gray-100 text-gray-500 border-gray-200"
    : "bg-blue-50 text-blue-700 border-blue-200"
  return (
    <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${styles()}`}>
      {props.planId}
    </span>
  )
}

// Presentational header for RelayDetailCard: icon/title/status, the WSS link, the
// optional tenant profile link, and the actions dropdown.
// All relay mutations are surfaced as callbacks. Props are reactive only when
// read lazily, so access props.* (and accessor props like props.relay()) inside
// JSX, never destructure at the top. A purged relay is terminal, so its actions
// menu is hidden entirely.
type RelayCardHeaderProps = {
  relay: () => Relay
  showTenant?: boolean
  tenantProfile: () => ProfileContent | undefined
  editHref?: string
  deactivating?: boolean
  reactivating?: boolean
  purging?: boolean
  onRequestDeactivate?: () => void
  onRequestReactivate?: () => void
  onRequestManageCustomDomain?: () => void
  onRequestTransferData?: () => void
  onRequestPurge?: () => void
}

export default function RelayCardHeader(props: RelayCardHeaderProps) {
  const r = () => props.relay()
  const metadata = () => props.tenantProfile()

  const showMenu = () =>
    r().purged_at == null &&
    (props.editHref || props.onRequestDeactivate || props.onRequestReactivate ||
      props.onRequestManageCustomDomain || props.onRequestTransferData || props.onRequestPurge)

  return (
    <>
      {/* Header */}
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-start gap-4 min-w-0">
          <Show when={r().info_icon}>
            <img src={r().info_icon} alt="" class="w-14 h-14 rounded-xl object-cover shrink-0 border border-gray-200" />
          </Show>
          <div class="min-w-0">
            <div class="flex items-center gap-3 flex-wrap">
              <h1 class="text-2xl font-bold text-gray-900">{r().info_name || r().subdomain}</h1>
              <StatusBadge status={relayStatus(r())} />
              <PlanBadge planId={r().plan_id} />
            </div>
            <a
              href={`https://${r().subdomain}.${r().zooid_domain}`}
              class="text-sm text-blue-600 hover:underline break-all"
            >
              wss://{r().subdomain}.{r().zooid_domain}
            </a>
            <Show when={props.showTenant}>
              <A href={`/admin/tenants/${r().tenant_pubkey}`} class="group mt-1.5 flex w-fit items-center gap-2 min-w-0">
                <Show
                  when={getProfilePicture(metadata())}
                  fallback={
                    <div class="h-6 w-6 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                      {((metadata()?.name || metadata()?.display_name) || r().tenant_pubkey).slice(0, 1).toUpperCase()}
                    </div>
                  }
                >
                  <img src={getProfilePicture(metadata())} alt="" class="h-6 w-6 flex-shrink-0 rounded-full object-cover" />
                </Show>
                <Show
                  when={metadata()?.name || metadata()?.display_name}
                  fallback={<CopyNpub pubkey={r().tenant_pubkey} class="text-sm text-blue-600 group-hover:underline" />}
                >
                  <span class="text-sm text-blue-600 group-hover:underline truncate">{metadata()?.name || metadata()?.display_name}</span>
                </Show>
              </A>
            </Show>
            <Show when={r().info_description.trim()}>
              <p class="mt-2 text-sm text-gray-600">{r().info_description}</p>
            </Show>
          </div>
        </div>

        <Show when={showMenu()}>
          <ActionsMenu>
            {(close) => (
              <>
                <Show when={props.editHref}>
                  <A
                    href={props.editHref!}
                    class="block px-3 py-2 text-sm text-gray-700 hover:bg-gray-50"
                    onClick={close}
                  >
                    Edit Details
                  </A>
                </Show>
                <Show when={props.onRequestManageCustomDomain}>
                  <ActionsMenuItem
                    onClick={() => {
                      close()
                      props.onRequestManageCustomDomain?.()
                    }}
                  >
                    Manage custom domain
                  </ActionsMenuItem>
                </Show>
                <Show when={props.onRequestTransferData}>
                  <ActionsMenuItem
                    onClick={() => {
                      close()
                      props.onRequestTransferData?.()
                    }}
                  >
                    Import / export data
                  </ActionsMenuItem>
                </Show>
                <Show when={r().status === "active" && props.onRequestDeactivate}>
                  <ActionsMenuItem
                    tone="danger"
                    disabled={props.deactivating}
                    onClick={() => {
                      close()
                      props.onRequestDeactivate?.()
                    }}
                  >
                    {props.deactivating ? "Deactivating..." : "Deactivate"}
                  </ActionsMenuItem>
                </Show>
                <Show when={r().status === "inactive" && props.onRequestReactivate}>
                  <ActionsMenuItem
                    tone="primary"
                    disabled={props.reactivating}
                    onClick={() => {
                      close()
                      props.onRequestReactivate?.()
                    }}
                  >
                    {props.reactivating ? "Reactivating..." : "Reactivate"}
                  </ActionsMenuItem>
                </Show>
                <Show when={props.onRequestPurge}>
                  <ActionsMenuItem
                    tone="danger"
                    disabled={props.purging}
                    onClick={() => {
                      close()
                      props.onRequestPurge?.()
                    }}
                  >
                    {props.purging ? "Purging..." : "Purge relay"}
                  </ActionsMenuItem>
                </Show>
              </>
            )}
          </ActionsMenu>
        </Show>
      </div>
    </>
  )
}
