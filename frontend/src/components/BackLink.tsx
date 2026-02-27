import { A } from "@solidjs/router"

type BackLinkProps = {
  href: string
  label: string
}

export default function BackLink(props: BackLinkProps) {
  return (
    <div class="flex items-center gap-2 mb-6">
      <A href={props.href} class="text-gray-500 hover:text-gray-700">← {props.label}</A>
    </div>
  )
}
