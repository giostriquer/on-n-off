import type { LoginState, SavedProfile } from "$lib/accountTypes";
import { formatExactDate, formatShortDate, parseInstant } from "$lib/limitsFormat";
import { providerLabel, type LimitsProvider } from "$lib/providers";
import type { TermTone } from "./TermBadge";
import { TERM_WARNING_WINDOW_MS, termCountdown } from "./termCountdown";

export type SignInPresentation = {
  state: "ending" | "ended" | "refused" | "otherAccount";
  name: string;
  label: string;
  tone: Extract<TermTone, "warning" | "expired">;
  progress: number;
  headline: string;
  countdown: string | null;
  next: string;
  message: string | null;
};

const USABLE: LoginState = { state: "usable" };
const AGAIN = "Sign in again to use this account.";

export function loginStateOf(profile: SavedProfile, now: number): LoginState {
  const login = profile.loginState ?? USABLE;
  if (login.state !== "ending") return login;
  const until = parseInstant(login.endsAt);
  return until !== null && until <= now ? { state: "ended", endsAt: login.endsAt } : login;
}

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
        state: "refused", name: `${name} sign-in refused`, label: "Sign-in refused", tone: "expired", progress: 0,
        headline: `${name} no longer accepts this saved login`, countdown: null, next: AGAIN,
        message: `${name} no longer accepts this saved login. ${AGAIN}`,
      };
    case "otherAccount":
      return {
        state: "otherAccount", name: `${name} sign-in is another account`, label: "Other account", tone: "expired", progress: 0,
        headline: `This saved login now signs in as a different ${name} account`, countdown: null, next: AGAIN,
        message: `This saved login now signs in as a different ${name} account. ${AGAIN}`,
      };
    case "ending":
    case "ended":
      return deadline(name, login.endsAt, now, login.state === "ended");
  }
}

function deadline(name: string, endsAt: string, now: number, ended: boolean): SignInPresentation | null {
  const until = parseInstant(endsAt);
  if (until === null) return null;
  const remaining = until - now;
  if (!ended && remaining > TERM_WARNING_WINDOW_MS) return null;
  const short = formatShortDate(endsAt, { yearUnlessSameAs: now });
  const exact = formatExactDate(endsAt);
  if (ended || remaining <= 0) {
    return {
      state: "ended", name: `${name} sign-in ended ${short}`, label: "Sign-in ended", tone: "expired", progress: 0,
      headline: `${name} ended this sign-in ${exact}`, countdown: null, next: AGAIN,
      message: `${name} ended this sign-in on ${short}. ${AGAIN}`,
    };
  }
  const { progress, countdown } = termCountdown(remaining);
  return {
    state: "ending", name: `${name} sign-in ends ${short}`, label: `Sign-in ends ${short}`, tone: "warning", progress,
    headline: `${name} ends this sign-in ${exact}`,
    countdown,
    next: `Then ${name} asks you to sign in again.`,
    message: null,
  };
}
