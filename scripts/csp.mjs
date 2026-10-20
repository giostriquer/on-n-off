export function cspHeader(policy) {
  return Object.entries(policy)
    .map(([directive, value]) => [directive, ...[value].flat().flatMap((source) => String(source).split(/\s+/)).filter(Boolean)].join(" "))
    .join("; ");
}
