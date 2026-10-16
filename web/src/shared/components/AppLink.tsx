import { A } from "@solidjs/router"
import { splitProps, type JSX } from "solid-js"

const isAdminPath = (path: string) => path === "/admin" || path.startsWith("/admin/")

// A link within this dashboard, or a page load into the other one: the tenant
// and admin dashboards are separate apps.
export default function AppLink(props: JSX.AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }) {
  const [local, rest] = splitProps(props, ["href"])

  return isAdminPath(local.href) === isAdminPath(window.location.pathname)
    ? <A href={local.href} {...rest} />
    : <a href={local.href} {...rest} />
}
