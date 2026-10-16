import { createSignal } from "solid-js"
import type { IAccount } from "applesauce-accounts"

// The account signed in to the dashboards, if any.
export const [account, setAccount] = createSignal<IAccount | undefined>()
