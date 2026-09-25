/** The end of a Codex account's paid period, as its login's ID token says. The token carries no renewal status. */
export type SubscriptionDate = {
  date: string;
  checkedAt: string | null;
};
