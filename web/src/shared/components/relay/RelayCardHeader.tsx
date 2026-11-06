import { A } from "@solidjs/router"
import { Show } from "solid-js"
import type { ProfileContent } from "applesauce-core/helpers/profile"
import { canonicalHost, relayName, relayStatus, type Relay } from "@/shared/lib/api"
import { FREE } from "@/shared/lib/plans"
import { profileName } from "@/shared/lib/profiles"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import Avatar from "@/shared/components/Avatar"
import CopyNpub from "@/shared/components/CopyNpub"
import StatusBadge from "@/shared/components/StatusBadge"

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

type RelayCardHeaderProps = {
  relay: () => Relay
  showTenant?: boolean
  tenantProfile: () => ProfileContent | undefined
  editHref?: string
  busy?: boolean
  onRequestDeactivate?: () => void
  onRequestReactivate?: () => void
  onRequestManageCustomDomain?: () => void
  onRequestTransferData?: () => void
  onRequestErase?: () => void
  onRequestPurge?: () => void
}

// A relay's name, status, address and owner, with an actions menu unless it's purged.
export default function RelayCardHeader(props: RelayCardHeaderProps) {
  const r = () => props.relay()
  const metadata = () => props.tenantProfile()

  const showMenu = () =>
    r().purged_at == null &&
    (props.editHref || props.onRequestDeactivate || props.onRequestReactivate ||
      props.onRequestManageCustomDomain || props.onRequestTransferData || props.onRequestErase ||
      props.onRequestPurge)

  return (
    <div class="flex items-start justify-between gap-4">
      <div class="flex items-start gap-4 min-w-0">
        <Show when={r().info_icon}>
          <img src={r().info_icon} alt="" class="w-14 h-14 rounded-xl object-cover shrink-0 border border-gray-200" />
        </Show>
        <div class="min-w-0">
          <div class="flex items-center gap-3 flex-wrap">
            <h1 class="text-2xl font-bold text-gray-900">{relayName(r())}</h1>
            <StatusBadge status={relayStatus(r())} />
            <PlanBadge planId={r().plan_id} />
          </div>
          <a
            href={`https://${canonicalHost(r())}`}
            class="text-sm text-blue-600 hover:underline break-all"
          >
            wss://{canonicalHost(r())}
          </a>
          <Show when={props.showTenant}>
            <A href={`/tenants/${r().tenant_pubkey}`} class="group mt-1.5 flex w-fit items-center gap-2 min-w-0">
              <Avatar profile={metadata()} pubkey={r().tenant_pubkey} class="h-6 w-6 flex-shrink-0" fallbackClass="bg-gray-200 text-[10px] text-gray-500" />
              <Show
                when={profileName(metadata())}
                fallback={<CopyNpub pubkey={r().tenant_pubkey} class="text-sm text-blue-600 group-hover:underline" />}
              >
                {(name) => <span class="text-sm text-blue-600 group-hover:underline truncate">{name()}</span>}
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
                  disabled={props.busy}
                  onClick={() => {
                    close()
                    props.onRequestDeactivate?.()
                  }}
                >
                  {props.busy ? "Deactivating..." : "Deactivate"}
                </ActionsMenuItem>
              </Show>
              <Show when={r().status === "inactive" && props.onRequestReactivate}>
                <ActionsMenuItem
                  tone="primary"
                  disabled={props.busy}
                  onClick={() => {
                    close()
                    props.onRequestReactivate?.()
                  }}
                >
                  {props.busy ? "Reactivating..." : "Reactivate"}
                </ActionsMenuItem>
              </Show>
              <Show when={props.onRequestErase}>
                <ActionsMenuItem
                  tone="danger"
                  disabled={props.busy}
                  onClick={() => {
                    close()
                    props.onRequestErase?.()
                  }}
                >
                  {props.busy ? "Erasing..." : "Erase relay"}
                </ActionsMenuItem>
              </Show>
              <Show when={props.onRequestPurge}>
                <ActionsMenuItem
                  tone="danger"
                  disabled={props.busy}
                  onClick={() => {
                    close()
                    props.onRequestPurge?.()
                  }}
                >
                  {props.busy ? "Deleting..." : "Delete relay"}
                </ActionsMenuItem>
              </Show>
            </>
          )}
        </ActionsMenu>
      </Show>
    </div>
  )
}
