import type { LoginState } from "$lib/accountTypes";
import { formatExactDate, formatShortDate, parseInstant } from "$lib/limitsFormat";
import { providerClient, providerLabel, type LimitsProvider } from "$lib/providers";
import type { TermTone } from "./TermBadge";
import { TERM_WARNING_WINDOW_MS, termCountdown } from "./termCountdown";

export type SignInPresentation = {
  name: string;
  label: string;
  tone: Extract<TermTone, "warning" | "expired">;
  progress: number;
  headline: string;
  countdown: string | null;
  next: string;
  message: string | null;
};

const AGAIN = "Sign in again to use this account.";

export function asksToSignInAgain(login: LoginState | null): boolean {
  return login?.state === "ended" || login?.state === "refused" || login?.state === "otherAccount";
}

export function signInPresentation(login: LoginState, provider: LimitsProvider, now: number): SignInPresentation | null {
  const name = providerLabel(provider);
  switch (login.state) {
    case "usable":
    case "missing":
      return null;
    case "refused":
      return {
        name: `${name} sign-in refused`, label: "Sign-in refused", tone: "expired", progress: 0,
        headline: `${name} no longer accepts this saved login`, countdown: null, next: AGAIN,
        message: `${name} no longer accepts this saved login. ${AGAIN}`,
      };
    case "otherAccount":
      return {
        name: `${name} sign-in is another account`, label: "Other account", tone: "expired", progress: 0,
        headline: `This saved login now signs in as a different ${name} account`, countdown: null, next: AGAIN,
        message: `This saved login now signs in as a different ${name} account. ${AGAIN}`,
      };
    case "ended":
      return ended(name, login.endsAt, now);
    case "ending":
      return ending(name, providerClient(provider), login.endsAt, now);
  }
}

function ended(name: string, endsAt: string, now: number): SignInPresentation {
  const short = formatShortDate(endsAt, { yearUnlessSameAs: now });
  return {
    name: `${name} sign-in ended ${short}`, label: "Sign-in ended", tone: "expired", progress: 0,
    headline: `${name} ended this sign-in ${formatExactDate(endsAt)}`, countdown: null, next: AGAIN,
    message: `${name} ended this sign-in on ${short}. ${AGAIN}`,
  };
}

function ending(name: string, client: string, endsAt: string, now: number): SignInPresentation | null {
  const until = parseInstant(endsAt);
  if (until === null || until - now > TERM_WARNING_WINDOW_MS) return null;
  const short = formatShortDate(endsAt, { yearUnlessSameAs: now });
  const { progress, countdown } = termCountdown(until - now);
  return {
    name: `${name} sign-in ends ${short}`, label: `Sign-in ends ${short}`, tone: "warning", progress,
    headline: `${name} ends this sign-in ${formatExactDate(endsAt)}`,
    countdown,
    next: `Then ${client} asks you to sign in again.`,
    message: null,
  };
}
