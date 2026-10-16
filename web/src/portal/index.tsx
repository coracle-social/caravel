/* @refresh reload */
import { render } from "solid-js/web"
import "@/portal/index.css"
import App from "@/portal/App"
import { portalRelay } from "@/portal/lib/relay"

document.title = portalRelay()?.name ?? "Membership"

render(() => <App />, document.getElementById("root")!)
