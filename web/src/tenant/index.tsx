/* @refresh reload */
import { render } from "solid-js/web"
import "@/shared/index.css"
import App from "@/tenant/App"
import { PLATFORM_NAME } from "@/shared/lib/config"

document.title = PLATFORM_NAME

render(() => <App />, document.getElementById("root")!)
