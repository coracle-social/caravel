import { Show } from "solid-js"
import { getProfilePicture, type ProfileContent } from "applesauce-core/helpers/profile"
import { profileName } from "@/shared/lib/profiles"

// A profile picture, or else the name's first letter, sized by `class` and colored by `fallbackClass`.
export default function Avatar(props: { profile: ProfileContent | undefined; pubkey: string; class: string; fallbackClass: string; alt?: string }) {
  return (
    <Show
      when={getProfilePicture(props.profile)}
      fallback={
        <div class={`${props.class} rounded-full flex items-center justify-center ${props.fallbackClass}`}>
          {(profileName(props.profile) || props.pubkey).slice(0, 1).toUpperCase()}
        </div>
      }
    >
      {(src) => <img src={src()} alt={props.alt ?? ""} class={`${props.class} rounded-full object-cover`} />}
    </Show>
  )
}
