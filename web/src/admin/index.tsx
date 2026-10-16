/* @refresh reload */
import { render } from "solid-js/web"
import "@/shared/index.css"
import App from "@/admin/App"
import { PLATFORM_NAME } from "@/shared/lib/config"

document.title = `${PLATFORM_NAME} admin`

render(() => <App />, document.getElementById("root")!)
