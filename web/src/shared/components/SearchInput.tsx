type SearchInputProps = {
  value: string
  onInput: (value: string) => void
  placeholder?: string
}

export default function SearchInput(props: SearchInputProps) {
  return (
    <div class="relative">
      <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-gray-400">
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8" />
          <path d="M21 21l-4.3-4.3" />
        </svg>
      </span>
      <input
        type="search"
        value={props.value}
        onInput={(e) => props.onInput(e.currentTarget.value)}
        placeholder={props.placeholder ?? "Search..."}
        class="w-full border border-gray-300 rounded-lg py-2 pl-10 pr-3 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />
    </div>
  )
}
