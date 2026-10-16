const SUBDOMAIN_LABEL_MAX_LEN = 63
const RESERVED_SUBDOMAIN_LABELS = new Set(["api", "admin"])

export function validateSubdomainLabel(subdomain: string): string | null {
  if (subdomain.length === 0) {
    return "subdomain is required"
  }
  if (subdomain.length > SUBDOMAIN_LABEL_MAX_LEN) {
    return "subdomain must be 63 characters or fewer"
  }
  if (subdomain.startsWith("-") || subdomain.endsWith("-")) {
    return "subdomain cannot start or end with a hyphen"
  }
  if (RESERVED_SUBDOMAIN_LABELS.has(subdomain)) {
    return "subdomain is reserved"
  }
  if (!/^[a-z0-9-]+$/.test(subdomain)) {
    return "subdomain may only contain lowercase letters, numbers, and hyphens"
  }

  return null
}
