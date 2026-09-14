export type SubscriptionReading = {
  metadata: {
    date: string | null;
    kind: "renews" | "expires" | "paidThrough" | null;
    source: "billing" | "localToken";
    checkedAt: string | null;
    stale: boolean;
  } | null;
  /** No retained browser connection; kept for IPC compatibility. */
  connected: boolean;
  browserSupported?: boolean;
  canConnect?: boolean;
  unavailable: boolean;
};
