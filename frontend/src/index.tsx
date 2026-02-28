/* @refresh reload */
import { render } from "solid-js/web"
import "./index.css"
import App from "./App"
import { PLATFORM_NAME, restoreAccounts } from "./lib/nostr"

const root = document.getElementById("root")!

document.title = PLATFORM_NAME

import("preline").then(() => {
  restoreAccounts()
  render(() => <App />, root)
})
