import { Show } from "solid-js"
import Avatar from "@/shared/components/Avatar"
import CopyNpub from "@/shared/components/CopyNpub"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"

// A pubkey as its avatar and name, with its npub to copy.
export default function PubkeyLabel(props: { pubkey: string }) {
  const metadata = useProfileMetadata(() => props.pubkey)

  return (
    <div class="flex min-w-0 items-center gap-2">
      <Avatar profile={metadata()} pubkey={props.pubkey} class="h-8 w-8 shrink-0" fallbackClass="bg-gray-200 text-xs text-gray-500" />
      <div class="min-w-0">
        <Show when={profileName(metadata())}>
          {(name) => <p class="truncate text-sm font-medium text-gray-900">{name()}</p>}
        </Show>
        <CopyNpub pubkey={props.pubkey} class="text-xs text-gray-500" />
      </div>
    </div>
  )
}
