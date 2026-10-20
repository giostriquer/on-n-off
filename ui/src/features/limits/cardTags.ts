import type { SignInPresentation } from "./loginState";
import type { CardSubscription, LimitCard } from "./limitCards";
import type { CardStatus } from "./limitPresentation";

export type CardTag =
  | { kind: "term"; subscription: CardSubscription }
  | { kind: "signIn"; signIn: SignInPresentation }
  | { kind: "usageStatus"; detail: string }
  | { kind: "remembered" }
  | { kind: "paused" }
  | { kind: "plan"; text: string };

export function cardTags(card: LimitCard): CardTag[] {
  const tags: CardTag[] = [];
  if (card.status) tags.push(statusTag(card.status));
  if (card.subscription) tags.push({ kind: "term", subscription: card.subscription });
  if (card.signIn) tags.push({ kind: "signIn", signIn: card.signIn });
  if (card.plan) tags.push({ kind: "plan", text: card.plan });
  return tags;
}

function statusTag(status: CardStatus): CardTag {
  switch (status.kind) {
    case "savedRefresh": return { kind: "usageStatus", detail: status.detail };
    case "remembered": return { kind: "remembered" };
    case "paused": return { kind: "paused" };
  }
}

export function unhandledTag(tag: never): never {
  throw new Error(`unhandled card tag ${JSON.stringify(tag)}`);
}
