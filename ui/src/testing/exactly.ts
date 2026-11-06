export function exactly(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error(`${JSON.stringify(value)} is not an object`);
  const found = Object.keys(value).sort();
  if (found.join() !== [...keys].sort().join()) throw new Error(`${JSON.stringify(value)} does not hold exactly ${keys.join(", ")}`);
  return value as Record<string, unknown>;
}
