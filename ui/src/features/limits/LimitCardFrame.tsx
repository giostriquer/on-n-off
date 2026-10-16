import type { ReactNode, RefCallback } from "react";
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { ProviderIcon } from "$lib/ProviderIcon";
import type { LimitsProvider } from "$lib/providers";
import { checkLimitsNow, limitsKey } from "$lib/sharedReads/limits";
import { CardMenu, type CardMenuItem } from "./CardMenu";
import { cardTags, unhandledTag, type CardTag } from "./cardTags";
import type { LimitCard } from "./limitCards";
import { SignInBadge } from "./SignInBadge";
import { AccountSubscriptionBadge } from "./SubscriptionBadge";
import { UsageStatusBadge } from "./UsageStatusBadge";
import { useTagsBeside } from "./useTagsBeside";

export function LimitCardFrame({ provider, card, title, ariaLabel, status, now, menuLabel, items = [], menuButtonRef, footer, children }: {
  provider: LimitsProvider;
  card?: LimitCard;
  title: string;
  ariaLabel: string;
  status: string;
  now: number;
  menuLabel: string;
  items?: CardMenuItem[];
  menuButtonRef?: RefCallback<HTMLButtonElement>;
  footer?: ReactNode;
  children?: ReactNode;
}) {
  const checkNow = useCheckLimitsNow(provider);
  return (
    <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label={ariaLabel} data-status={status}>
      <CardHeader card={card} provider={provider} title={title} now={now}
        menu={<CardMenu label={menuLabel} items={[checkNow, ...items]} buttonRef={menuButtonRef} />} />
      {children}
      <footer className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5 empty:hidden">{footer}</footer>
    </section>
  );
}

function useCheckLimitsNow(provider: LimitsProvider): CardMenuItem {
  const client = useQueryClient();
  const checking = useIsFetching({ queryKey: limitsKey(provider) }) > 0;
  return { id: "checkNow", label: "Check limits now", disabled: checking, run: () => void checkLimitsNow(client, provider).catch(() => {}) };
}

function CardHeader({ card, provider, title, now, menu }: { card?: LimitCard; provider: LimitsProvider; title: string; now: number; menu: ReactNode }) {
  const updatedAt = card?.freshness.updatedAt;
  const { beside, line, name, tags: tagsRow } = useTagsBeside();
  const tags = (
    <div ref={tagsRow} data-testid="account-tags" className={`flex flex-wrap items-center gap-1.5 whitespace-nowrap empty:hidden ${beside ? "" : "mt-1 pl-6"}`}>
      {card && cardTags(card).map(tag => <HeaderTag key={tag.kind} tag={tag} now={now} />)}
    </div>
  );
  return (
    <header className="border-b border-[var(--hair)] px-3.5 py-2.5" title={updatedAt ? `Usage last checked ${updatedAt}` : undefined}>
      <div className="flex items-center gap-2.5">
        <ProviderIcon provider={provider} className="size-3.5 shrink-0 translate-y-[0.5px]" />
        <div ref={line} className="flex min-w-0 flex-1 items-center gap-2.5">
          <div ref={name} className="min-w-0 truncate text-[13px] font-semibold" title={title}>{title}</div>
          {beside && tags}
        </div>
        {card?.active && !card.headline && <ActiveAccountDot />}
        {menu}
      </div>
      {!beside && tags}
      {card?.identity.category && <div className="mt-0.5 break-words pl-6 text-[11px] text-[var(--mute)]">{card.identity.category}</div>}
      {updatedAt ? <p className="sr-only">Latest observation {updatedAt}</p> : null}
    </header>
  );
}

function HeaderTag({ tag, now }: { tag: CardTag; now: number }) {
  switch (tag.kind) {
    case "term": return <AccountSubscriptionBadge subscription={tag.subscription} now={now} />;
    case "signIn": return <SignInBadge signIn={tag.signIn} />;
    case "usageStatus": return <UsageStatusBadge detail={tag.detail} />;
    case "remembered":
    case "paused": return null;
    case "plan": return <span className="rounded-md border border-[var(--hair)] px-1.5 py-0.5 type-badge uppercase">{tag.text}</span>;
    default: return unhandledTag(tag);
  }
}

export function ActiveAccountDot() {
  return <span role="img" aria-label="Active account" title="Active account"
    className="size-[7px] shrink-0 rounded-full bg-[var(--live)] shadow-[0_0_8px_var(--live)]" />;
}
