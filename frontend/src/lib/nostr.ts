import { EventStore } from "applesauce-core"
import { RelayPool } from "applesauce-relay"
import { AccountManager } from "applesauce-accounts"

export const eventStore = new EventStore()
export const pool = new RelayPool()
export const accounts = new AccountManager()
