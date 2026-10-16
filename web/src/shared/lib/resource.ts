import type { Resource } from "solid-js"

// A resource's value, or undefined while it loads or once it has failed, when reading it would throw.
export function loaded<T>(resource: Resource<T>): T | undefined {
  return resource.error ? undefined : resource()
}
