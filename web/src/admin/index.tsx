/* @refresh reload */
import { render } from "solid-js/web"
import "@/shared/index.css"
import App from "@/admin/App"
import { PLATFORM_NAME } from "@/shared/lib/config"

const root = document.getElementById("root")!

document.title = `${PLATFORM_NAME} admin`

import("preline").then(() => {
  render(() => <App />, root)
})
