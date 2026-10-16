import type { SignInPresentation } from "./claudeSignIn";
import type { CardSubscription, LimitCard } from "./limitCards";

export type CardTag =
  | { kind: "term"; subscription: CardSubscription }
  | { kind: "signIn"; signIn: SignInPresentation }
  | { kind: "usageStatus"; detail: string }
  | { kind: "remembered" }
  | { kind: "paused" }
  | { kind: "plan"; text: string };

export function cardTags(card: LimitCard): CardTag[] {
  const tags: CardTag[] = [];
  if (card.subscription) tags.push({ kind: "term", subscription: card.subscription });
  if (card.signIn) tags.push({ kind: "signIn", signIn: card.signIn });
  if (card.status?.kind === "savedRefresh") tags.push({ kind: "usageStatus", detail: card.status.detail });
  if (card.status?.kind === "remembered") tags.push({ kind: "remembered" });
  if (card.status?.kind === "paused") tags.push({ kind: "paused" });
  if (card.plan) tags.push({ kind: "plan", text: card.plan });
  return tags;
}

export function unhandledTag(tag: never): never {
  throw new Error(`unhandled card tag ${JSON.stringify(tag)}`);
}
