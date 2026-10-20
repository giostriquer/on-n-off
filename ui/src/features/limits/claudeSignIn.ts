import { formatExactDate, formatShortDate, parseInstant } from "$lib/limitsFormat";
import type { TermTone } from "./TermBadge";
import { TERM_WARNING_WINDOW_MS, termCountdown } from "./termCountdown";

export type SignInPresentation = {
  state: "ending" | "ended";
  name: string;
  label: string;
  tone: Extract<TermTone, "warning" | "expired">;
  progress: number;
  headline: string;
  countdown: string | null;
  next: string;
  message: string | null;
};

export function claudeSignIn(endsAt: string | null | undefined, now: number): SignInPresentation | null {
  const until = parseInstant(endsAt);
  if (until === null) return null;
  const remaining = until - now;
  if (remaining > TERM_WARNING_WINDOW_MS) return null;
  const short = formatShortDate(endsAt, { yearUnlessSameAs: now });
  const exact = formatExactDate(endsAt);
  if (remaining <= 0) {
    const next = "Sign in again to use this account.";
    return {
      state: "ended", name: `Claude sign-in ended ${short}`, label: "Sign-in ended", tone: "expired", progress: 0,
      headline: `Claude ended this sign-in ${exact}`, countdown: null, next,
      message: `Claude ended this sign-in on ${short}. ${next}`,
    };
  }
  const { progress, countdown } = termCountdown(remaining);
  return {
    state: "ending", name: `Claude sign-in ends ${short}`, label: `Sign-in ends ${short}`, tone: "warning", progress,
    headline: `Claude ends this sign-in ${exact}`,
    countdown,
    next: "Then Claude Code asks you to sign in again.",
    message: null,
  };
}
