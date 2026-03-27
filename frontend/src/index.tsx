/* @refresh reload */
import { render } from "solid-js/web"
import "./index.css"
import App from "./App"
import { PLATFORM_NAME } from "./lib/hooks"

const root = document.getElementById("root")!

document.title = PLATFORM_NAME

import("preline").then(() => {
  render(() => <App />, root)
})
