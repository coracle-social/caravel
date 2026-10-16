import type { JSX } from "solid-js"

type PageContainerProps = {
  children: JSX.Element
  size?: "narrow" | "wide"
}

export default function PageContainer(props: PageContainerProps) {
  const maxWidthClass = () => (props.size === "narrow" ? "max-w-2xl" : "max-w-4xl")

  return <div class={`${maxWidthClass()} mx-auto px-4 py-8`}>{props.children}</div>
}
