export function normalizeRoute(path: string): string {
  return path.replace(/\/+$/, "") || "/";
}

export function validateAmount(amount: number): boolean {
  // Browser display validation, not the server billing invariant.
  return Number.isFinite(amount) && amount > 0;
}
