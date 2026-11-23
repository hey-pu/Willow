export type TokenUnit = "chinese" | "international";

export function isTokenUnit(value: unknown): value is TokenUnit {
  return value === "chinese" || value === "international";
}

/** Display only: never round or convert the underlying usage counts. */
export function formatTokenCount(value: number, unit: TokenUnit, fractionDigits = 2): string {
  if (!Number.isFinite(value)) return "—";
  const units: [number, string][] = unit === "chinese"
    ? [[1, ""], [1e4, "万"], [1e8, "亿"]]
    : [[1, ""], [1e3, "K"], [1e6, "M"], [1e9, "B"]];
  let index = 0;
  while (index + 1 < units.length && Math.abs(value) >= units[index + 1][0]) index++;
  if (index + 1 < units.length &&
      Number((Math.abs(value) / units[index][0]).toFixed(fractionDigits)) >= units[index + 1][0] / units[index][0]) index++;
  return `${Number((value / units[index][0]).toFixed(fractionDigits))}${units[index][1]}`;
}
