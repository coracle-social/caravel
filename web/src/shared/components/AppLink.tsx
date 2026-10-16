import { A } from "@solidjs/router"
import { splitProps, type JSX } from "solid-js"

// Whether a path belongs to the admin dashboard rather than the tenant one.
export const isAdminPath = (path: string) => path === "/admin" || path.startsWith("/admin/")

// A link within this dashboard, or a page load into the other one, a separate app.
export default function AppLink(props: JSX.AnchorHTMLAttributes<HTMLAnchorElement> & { href: string }) {
  const [local, rest] = splitProps(props, ["href"])

  return isAdminPath(local.href) === isAdminPath(window.location.pathname)
    ? <A href={local.href} {...rest} />
    : <a href={local.href} {...rest} />
}
