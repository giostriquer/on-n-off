import type { ReactNode, RefCallback } from "react";
import { ActionMenu, type ActionMenuItem } from "@/components/ActionMenu";
import { ProviderIcon } from "$lib/ProviderIcon";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import { checkNowEntry } from "./cardActionPlan";
import { useCardActions, useCheckNow, type CardFrameInput } from "./cardActions";
import { cardTags, unhandledTag, type CardTag } from "./cardTags";
import { isSignedIn, type LimitCard } from "./limitCards";
import { SignInBadge } from "./SignInBadge";
import { AccountSubscriptionBadge } from "./SubscriptionBadge";
import { UsageStatusBadge } from "./UsageStatusBadge";
import { useTagsBeside } from "./useTagsBeside";

type FrameChrome = { menuButtonRef?: RefCallback<HTMLButtonElement>; children?: ReactNode };

export function LimitCardFrame({ menuButtonRef, children, ...input }: CardFrameInput & FrameChrome) {
  const { menu, footer } = useCardActions(input);
  return <CardShell provider={input.provider} card={input.card} now={input.now} menu={menu} footer={footer} menuButtonRef={menuButtonRef}>{children}</CardShell>;
}

export function PendingCardFrame({ provider, now, checking, menuButtonRef, children }: { provider: LimitsProvider; now: number; checking: boolean } & FrameChrome) {
  const checkNow = useCheckNow(provider);
  return <CardShell provider={provider} card={null} now={now} menu={[checkNow(checkNowEntry(checking))]} footer={null} menuButtonRef={menuButtonRef}>{children}</CardShell>;
}

function CardShell({ provider, card, now, menu, footer, menuButtonRef, children }: {
  provider: LimitsProvider; card: LimitCard | null; now: number; menu: ActionMenuItem[]; footer: ReactNode;
} & FrameChrome) {
  const name = providerLabel(provider);
  const title = card?.identity.label ?? name;
  return (
    <section className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]" aria-label={card?.identity.ariaLabel ?? `${name} limits`} data-status={card?.freshness.readStatus ?? "pending"}>
      <CardHeader card={card} provider={provider} title={title} now={now}
        menu={<ActionMenu label={card?.account?.name ?? title} items={menu} buttonRef={menuButtonRef} />} />
      {children}
      <footer className="flex flex-wrap items-center gap-2 border-t border-[var(--hair)] px-3.5 py-2.5 empty:hidden">{footer}</footer>
    </section>
  );
}

function CardHeader({ card, provider, title, now, menu }: { card: LimitCard | null; provider: LimitsProvider; title: string; now: number; menu: ReactNode }) {
  const updatedAt = card?.freshness.updatedAt;
  const { beside, line, name, tags: tagsRow } = useTagsBeside();
  const tags = (
    <div ref={tagsRow} data-testid="account-tags" className={`flex flex-wrap items-center gap-1.5 whitespace-nowrap empty:hidden ${beside ? "ml-auto" : "col-[2/-1] mt-1"}`}>
      {card && cardTags(card).map(tag => <HeaderTag key={tag.kind} tag={tag} now={now} />)}
    </div>
  );
  return (
    <header className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-2.5 border-b border-[var(--hair)] px-3.5 py-2.5" title={updatedAt ? `Usage last checked ${updatedAt}` : undefined}>
      <ProviderIcon provider={provider} className="size-3.5 shrink-0 translate-y-[0.5px]" />
      <div ref={line} className="flex min-w-0 items-center gap-2.5">
        <div ref={name} className="min-w-0 truncate text-[13px] font-semibold" title={title}>{title}</div>
        {beside && tags}
      </div>
      <div className="flex items-center gap-2.5">
        {card && isSignedIn(card) && !card.headline && <ActiveAccountDot />}
        {menu}
      </div>
      {!beside && tags}
      {card?.identity.category && <div data-testid="account-category" className="col-[2/-1] mt-0.5 break-words text-[11px] text-[var(--mute)]">{card.identity.category}</div>}
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
