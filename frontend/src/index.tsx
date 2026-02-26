/* @refresh reload */
import { render } from "solid-js/web"
import "./index.css"
import App from "./App"
import { restoreAccounts } from "./lib/nostr"

const root = document.getElementById("root")!

import("preline").then(() => {
  restoreAccounts()
  render(() => <App />, root)
})
