/* @refresh reload */
import { render } from "solid-js/web"
import "@/shared/index.css"
import App from "@/tenant/App"
import { PLATFORM_NAME } from "@/shared/lib/config"

const root = document.getElementById("root")!

document.title = PLATFORM_NAME

import("preline").then(() => {
  render(() => <App />, root)
})
